//! Typing SQL expressions and inferring parameters from predicates.
//!
//! [`infer`] gives a projected expression its neoc type and nullability —
//! columns, literals, arithmetic/concat/comparison, `CASE`, `CAST`, and a small
//! function-return catalog (aggregates, string/number builtins, `COALESCE`).
//! [`params`] walks a `WHERE`/`HAVING` predicate and records a [`Param`] for
//! each placeholder, typed from the column or expression it is compared with.

use sqlparser::ast::{
    BinaryOperator, Expr, Function, FunctionArg, FunctionArgExpr, FunctionArguments, UnaryOperator,
    Value,
};

use crate::catalog::last_ident;
use crate::query::Param;
use crate::scope::{Resolved, Scope};
use crate::{ColType, Error, Prim};

/// Infers the neoc type and nullability of a projected expression.
pub(crate) fn infer(query: &str, expr: &Expr, scope: &Scope) -> Result<Resolved, Error> {
    match expr {
        Expr::Identifier(ident) => scope.unqualified(query, &ident.value),
        Expr::CompoundIdentifier(parts) => match parts.as_slice() {
            [.., table, col] => scope.qualified(query, &table.value, &col.value),
            [col] => scope.unqualified(query, &col.value),
            [] => Err(unsupported(query, "empty column reference")),
        },
        Expr::Nested(inner) => infer(query, inner, scope),
        Expr::Value(value) => literal(query, value),
        Expr::UnaryOp { op, expr } => match op {
            UnaryOperator::Not => Ok(prim(Prim::Bool, nullable(query, expr, scope))),
            _ => Ok(prim(Prim::Number, nullable(query, expr, scope))),
        },
        Expr::BinaryOp { left, op, right } => {
            let n = nullable(query, left, scope) || nullable(query, right, scope);
            let p = match op {
                BinaryOperator::Plus
                | BinaryOperator::Minus
                | BinaryOperator::Multiply
                | BinaryOperator::Divide
                | BinaryOperator::Modulo => Prim::Number,
                BinaryOperator::StringConcat => Prim::String,
                _ => Prim::Bool,
            };
            Ok(prim(p, n))
        }
        Expr::Function(func) => function(query, func, scope),
        Expr::Case {
            results,
            else_result,
            ..
        } => {
            let first = results
                .first()
                .ok_or_else(|| unsupported(query, "CASE with no result branches"))?;
            let mut out = infer(query, first, scope)?;
            // NULL on any branch — or no ELSE (an unmatched CASE yields NULL) —
            // makes the result optional.
            let mut n = else_result.is_none();
            for r in results {
                n = n || nullable(query, r, scope);
            }
            if let Some(e) = else_result {
                n = n || nullable(query, e, scope);
            }
            out.nullable = n;
            Ok(out)
        }
        Expr::Cast {
            expr, data_type, ..
        } => Ok(Resolved {
            ty: ColType::from_sql(data_type),
            nullable: nullable(query, expr, scope),
        }),
        Expr::IsNull(_) | Expr::IsNotNull(_) => Ok(prim(Prim::Bool, false)),
        _ => Err(unsupported(
            query,
            "this expression is not supported in a SELECT list — alias a column, \
             an aggregate, or a simple expression",
        )),
    }
}

/// Walks a predicate, appending a [`Param`] for every placeholder in
/// left-to-right order. `AND`/`OR` recurse; a comparison, `BETWEEN`, `LIKE`, or
/// `IN (…)` against a placeholder types the parameter from the other operand.
pub(crate) fn params(
    query: &str,
    expr: &Expr,
    scope: &Scope,
    out: &mut Vec<Param>,
) -> Result<(), Error> {
    match expr {
        Expr::BinaryOp { left, op, right } => match op {
            BinaryOperator::And | BinaryOperator::Or => {
                params(query, left, scope, out)?;
                params(query, right, scope, out)?;
            }
            _ => {
                if is_placeholder(right) {
                    push_param(query, left, scope, out)?;
                } else if is_placeholder(left) {
                    push_param(query, right, scope, out)?;
                }
            }
        },
        Expr::Nested(inner) => params(query, inner, scope, out)?,
        Expr::UnaryOp { expr, .. } => params(query, expr, scope, out)?,
        Expr::Between {
            expr, low, high, ..
        } => {
            for bound in [low.as_ref(), high.as_ref()] {
                if is_placeholder(bound) {
                    push_param(query, expr, scope, out)?;
                }
            }
        }
        Expr::Like { expr, pattern, .. } | Expr::ILike { expr, pattern, .. }
            if is_placeholder(pattern) =>
        {
            push_param(query, expr, scope, out)?;
        }
        Expr::InList { expr, list, .. } => {
            for item in list {
                if is_placeholder(item) {
                    push_param(query, expr, scope, out)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Pushes a parameter — typed as the assigned column, named for it — for every
/// placeholder in an `UPDATE` `SET` value, so both `col = ?` and `col = col + ?`
/// bind correctly.
pub(crate) fn set_params(col: &str, as_type: Resolved, value: &Expr, out: &mut Vec<Param>) {
    match value {
        Expr::Value(Value::Placeholder(_)) => out.push(Param {
            name: col.to_string(),
            ty: as_type.ty,
            nullable: as_type.nullable,
        }),
        Expr::BinaryOp { left, right, .. } => {
            set_params(col, as_type, left, out);
            set_params(col, as_type, right, out);
        }
        Expr::UnaryOp { expr, .. } | Expr::Nested(expr) => set_params(col, as_type, expr, out),
        _ => {}
    }
}

/// Types and names a parameter from the operand opposite a placeholder.
fn push_param(query: &str, typed: &Expr, scope: &Scope, out: &mut Vec<Param>) -> Result<(), Error> {
    let resolved = infer(query, typed, scope)?;
    let name = column_name(typed).unwrap_or_else(|| "param".to_string());
    out.push(Param {
        name,
        ty: resolved.ty,
        nullable: resolved.nullable,
    });
    Ok(())
}

/// The neoc type of a SQL literal.
fn literal(query: &str, value: &Value) -> Result<Resolved, Error> {
    match value {
        Value::Number(_, _) => Ok(prim(Prim::Number, false)),
        Value::Boolean(_) => Ok(prim(Prim::Bool, false)),
        Value::Null => Err(unsupported(
            query,
            "a bare NULL in a SELECT list has no type — wrap it in a CAST or COALESCE",
        )),
        Value::Placeholder(_) => Err(unsupported(
            query,
            "a bind placeholder in a SELECT list cannot be typed — bind it in WHERE instead",
        )),
        // The remaining literals are the various string-quote styles.
        _ => Ok(prim(Prim::String, false)),
    }
}

/// The return type of a function call, from a small catalog. Unknown functions
/// fall back to their first argument's type (or `string`), nullable.
fn function(query: &str, func: &Function, scope: &Scope) -> Result<Resolved, Error> {
    let name = last_ident(&func.name).to_ascii_lowercase();
    let args = arg_types(query, func, scope)?;
    let arg_nullable = args.iter().flatten().any(|r| r.nullable);
    let first = args.first().and_then(|a| a.as_ref());
    // The type of the first argument, always nullable, defaulting `default`
    // when there is no typed argument — shared by min/max and the catch-all.
    let first_typed = |default: Prim| match first {
        Some(r) => Resolved {
            ty: r.ty,
            nullable: true,
        },
        None => prim(default, true),
    };
    Ok(match name.as_str() {
        "count" | "total" => prim(Prim::Number, false),
        "sum" | "avg" => prim(Prim::Number, true),
        "min" | "max" => first_typed(Prim::Number),
        "abs" | "round" | "floor" | "ceil" | "ceiling" | "length" | "char_length"
        | "character_length" | "octet_length" | "strftime" | "unixepoch" => {
            prim(Prim::Number, arg_nullable)
        }
        "upper" | "lower" | "trim" | "ltrim" | "rtrim" | "substr" | "substring" | "replace"
        | "concat" | "concat_ws" | "format" | "hex" | "quote" | "group_concat" | "datetime"
        | "date" | "time" => prim(Prim::String, arg_nullable),
        "coalesce" | "ifnull" => {
            // First well-typed argument; non-null unless every argument is.
            let ty = match first {
                Some(r) => r.ty,
                None => ColType::of(Prim::String),
            };
            Resolved {
                ty,
                nullable: args.iter().all(|a| a.as_ref().is_none_or(|r| r.nullable)),
            }
        }
        // `nullif` and any unknown function: first argument's type, nullable.
        _ => first_typed(Prim::String),
    })
}

/// Types each function argument; `None` for a `*` / `table.*` wildcard.
fn arg_types(query: &str, func: &Function, scope: &Scope) -> Result<Vec<Option<Resolved>>, Error> {
    let FunctionArguments::List(list) = &func.args else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for arg in &list.args {
        let arg_expr = match arg {
            FunctionArg::Unnamed(e) | FunctionArg::Named { arg: e, .. } => e,
        };
        match arg_expr {
            FunctionArgExpr::Expr(e) => out.push(Some(infer(query, e, scope)?)),
            FunctionArgExpr::Wildcard | FunctionArgExpr::QualifiedWildcard(_) => out.push(None),
        }
    }
    Ok(out)
}

/// The nullability of an expression, treating an untypable operand as non-null
/// (a best effort that never fails the whole query over a nested literal).
fn nullable(query: &str, expr: &Expr, scope: &Scope) -> bool {
    infer(query, expr, scope).is_ok_and(|r| r.nullable)
}

/// Whether an expression is a bind placeholder (`?`, `$1`, …).
pub(crate) fn is_placeholder(expr: &Expr) -> bool {
    matches!(expr, Expr::Value(Value::Placeholder(_)))
}

/// The column name of a plain or qualified column reference, if it is one.
pub(crate) fn column_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Identifier(ident) => Some(ident.value.clone()),
        Expr::CompoundIdentifier(parts) => parts.last().map(|i| i.value.clone()),
        Expr::Nested(inner) => column_name(inner),
        _ => None,
    }
}

/// A `Resolved` for a primitive with the natural `SqlValue` arm.
fn prim(p: Prim, nullable: bool) -> Resolved {
    Resolved {
        ty: ColType::of(p),
        nullable,
    }
}

fn unsupported(query: &str, reason: &str) -> Error {
    Error::Unsupported {
        name: query.to_string(),
        reason: reason.to_string(),
    }
}
