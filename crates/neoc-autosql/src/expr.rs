//! Typing SQL expressions and inferring parameters from predicates.
//!
//! [`infer`] gives a projected expression its neoc type and nullability —
//! columns, literals, arithmetic/concat/comparison, `CASE`, `CAST`, and a small
//! function-return catalog (aggregates, string/number builtins, `COALESCE`).
//! [`params`] walks a `WHERE`/`HAVING`/`JOIN … ON` predicate and records a [`Param`] for
//! each placeholder, typed from the column or expression it is compared with.
//!
//! Subqueries are typed here too. A scalar subquery `(SELECT …)` is its one
//! column's type and always nullable (an empty result is NULL); `EXISTS` is a
//! non-null `bool`; `x IN (SELECT …)` is a `bool`, nullable when `x` or the
//! subquery column can be NULL (SQL three-valued logic). A window function
//! (`f(…) OVER (…)`) is typed by [`window_function`].
//!
//! Typing and parameter collection are one pass: [`infer`] appends the
//! placeholders of every subquery it types, so each subquery is typed once
//! however deeply it nests.

use sqlparser::ast::{
    BinaryOperator, Expr, Function, FunctionArg, FunctionArgExpr, FunctionArguments, Query,
    UnaryOperator, Value, WindowSpec, WindowType,
};

use crate::catalog::last_ident;
use crate::query::{self, Param, ResultCol};
use crate::scope::{Projection, Resolved, Scope};
use crate::{ColType, Error, Prim};

/// Infers the neoc type and nullability of a projected expression, appending
/// the placeholders of the subqueries it contains to `out` in textual order.
pub(crate) fn infer(
    query: &str,
    expr: &Expr,
    scope: &Scope,
    out: &mut Vec<Param>,
) -> Result<Resolved, Error> {
    match expr {
        Expr::Identifier(ident) => scope.unqualified(query, &ident.value),
        Expr::CompoundIdentifier(parts) => match parts.as_slice() {
            [.., table, col] => scope.qualified(query, &table.value, &col.value),
            [col] => scope.unqualified(query, &col.value),
            [] => Err(unsupported(query, "empty column reference")),
        },
        Expr::Nested(inner) => infer(query, inner, scope, out),
        Expr::Value(value) => literal(query, value),
        Expr::UnaryOp { op, expr } => {
            let n = nullable(query, expr, scope, out)?;
            Ok(match op {
                UnaryOperator::Not => prim(Prim::Bool, n),
                _ => prim(Prim::Number, n),
            })
        }
        Expr::BinaryOp { left, op, right } => {
            let left_nullable = nullable(query, left, scope, out)?;
            let n = nullable(query, right, scope, out)? || left_nullable;
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
        Expr::Function(func) => function(query, func, scope, out),
        Expr::Case {
            operand,
            conditions,
            results,
            else_result,
        } => {
            if let Some(operand) = operand {
                subquery_params(query, operand, scope, out)?;
            }
            // The first branch gives the type; NULL on any branch — or no ELSE
            // (an unmatched CASE yields NULL) — makes the result optional.
            let mut typed: Option<Resolved> = None;
            let mut n = else_result.is_none();
            for (condition, result) in conditions.iter().zip(results) {
                subquery_params(query, condition, scope, out)?;
                if typed.is_none() {
                    let first = infer(query, result, scope, out)?;
                    n = n || first.nullable;
                    typed = Some(first);
                } else {
                    n = nullable(query, result, scope, out)? || n;
                }
            }
            if let Some(e) = else_result {
                n = nullable(query, e, scope, out)? || n;
            }
            let mut typed =
                typed.ok_or_else(|| unsupported(query, "CASE with no result branches"))?;
            typed.nullable = n;
            Ok(typed)
        }
        Expr::Cast {
            expr, data_type, ..
        } => Ok(Resolved {
            ty: ColType::from_sql(data_type),
            nullable: nullable(query, expr, scope, out)?,
        }),
        Expr::IsNull(expr) | Expr::IsNotNull(expr) => {
            subquery_params(query, expr, scope, out)?;
            Ok(prim(Prim::Bool, false))
        }
        Expr::Subquery(subquery) => {
            let column = single_column(query, subquery, scope, "a scalar subquery", out)?;
            Ok(Resolved {
                ty: column.ty,
                nullable: true,
            })
        }
        Expr::Exists { subquery, .. } => {
            subquery_columns(query, subquery, scope, Projection::Unread, out)?;
            Ok(prim(Prim::Bool, false))
        }
        Expr::InSubquery { expr, subquery, .. } => {
            let operand_nullable = nullable(query, expr, scope, out)?;
            let column = single_column(query, subquery, scope, "an IN subquery", out)?;
            Ok(prim(Prim::Bool, column.nullable || operand_nullable))
        }
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
            _ if is_placeholder(right) => {
                let param = typed_param(query, left, scope, out)?;
                out.push(param);
            }
            _ if is_placeholder(left) => {
                // The placeholder precedes the operand's own subqueries.
                let mut inner = Vec::new();
                let param = typed_param(query, right, scope, &mut inner)?;
                out.push(param);
                out.extend(inner);
            }
            _ => {
                subquery_params(query, left, scope, out)?;
                subquery_params(query, right, scope, out)?;
            }
        },
        Expr::Nested(inner) => params(query, inner, scope, out)?,
        Expr::UnaryOp { expr, .. } => params(query, expr, scope, out)?,
        Expr::Between {
            expr, low, high, ..
        } => operand_params(query, expr, [low.as_ref(), high.as_ref()], scope, out)?,
        Expr::Like { expr, pattern, .. } | Expr::ILike { expr, pattern, .. }
            if is_placeholder(pattern) =>
        {
            let param = typed_param(query, expr, scope, out)?;
            out.push(param);
        }
        Expr::InList { expr, list, .. } => operand_params(query, expr, list, scope, out)?,
        Expr::InSubquery { expr, subquery, .. } if is_placeholder(expr) => {
            // `? IN (SELECT …)`: the placeholder comes first textually, but its
            // type is the subquery's column, so type the subquery first.
            let mut inner = Vec::new();
            let column = single_column(query, subquery, scope, "an IN subquery", &mut inner)?;
            out.push(Param {
                name: column.field,
                ty: column.ty,
                nullable: column.nullable,
            });
            out.extend(inner);
        }
        _ => subquery_params(query, expr, scope, out)?,
    }
    Ok(())
}

/// Binds `expr BETWEEN …` / `expr IN (…)`: each placeholder among `others`
/// takes `expr`'s type and name; every other operand contributes only its
/// subqueries' placeholders. `expr` is typed once, ahead of the others, as it
/// comes first textually.
fn operand_params<'e>(
    query: &str,
    expr: &Expr,
    others: impl IntoIterator<Item = &'e Expr>,
    scope: &Scope,
    out: &mut Vec<Param>,
) -> Result<(), Error> {
    let others: Vec<&Expr> = others.into_iter().collect();
    let typed = if others.iter().any(|e| is_placeholder(e)) {
        Some(typed_param(query, expr, scope, out)?)
    } else {
        subquery_params(query, expr, scope, out)?;
        None
    };
    for other in others {
        match &typed {
            Some(param) if is_placeholder(other) => out.push(param.clone()),
            _ => subquery_params(query, other, scope, out)?,
        }
    }
    Ok(())
}

/// Appends the parameters of every subquery nested in `expr` — a predicate
/// operand, a `CASE` condition, a function argument — in textual order, without
/// typing `expr` itself. A placeholder outside a subquery is not collected
/// here: only `WHERE`/`HAVING`/`ON` comparisons ([`params`]) bind a bare
/// placeholder.
fn subquery_params(
    query: &str,
    expr: &Expr,
    scope: &Scope,
    out: &mut Vec<Param>,
) -> Result<(), Error> {
    match expr {
        Expr::Exists { subquery, .. } => {
            subquery_columns(query, subquery, scope, Projection::Unread, out)?;
        }
        Expr::Subquery(subquery) => {
            single_column(query, subquery, scope, "a scalar subquery", out)?;
        }
        Expr::InSubquery { expr, subquery, .. } => {
            subquery_params(query, expr, scope, out)?;
            single_column(query, subquery, scope, "an IN subquery", out)?;
        }
        _ => {
            for child in children(expr) {
                subquery_params(query, child, scope, out)?;
            }
        }
    }
    Ok(())
}

/// The direct sub-expressions of the expression forms a subquery can hide in,
/// in textual order. Forms not listed here have their subqueries rejected by
/// the whole-statement placeholder count, never silently mis-bound.
fn children(expr: &Expr) -> Vec<&Expr> {
    match expr {
        Expr::BinaryOp { left, right, .. } => vec![left, right],
        Expr::UnaryOp { expr, .. }
        | Expr::Nested(expr)
        | Expr::IsNull(expr)
        | Expr::IsNotNull(expr)
        | Expr::Cast { expr, .. } => vec![expr],
        Expr::Between {
            expr, low, high, ..
        } => vec![expr, low, high],
        Expr::InList { expr, list, .. } => std::iter::once(expr.as_ref()).chain(list).collect(),
        Expr::Like { expr, pattern, .. } | Expr::ILike { expr, pattern, .. } => {
            vec![expr, pattern]
        }
        Expr::Case {
            operand,
            conditions,
            results,
            else_result,
        } => operand
            .iter()
            .map(AsRef::as_ref)
            .chain(conditions.iter().zip(results).flat_map(|(c, r)| [c, r]))
            .chain(else_result.iter().map(AsRef::as_ref))
            .collect(),
        Expr::Function(func) => function_args(func),
        _ => Vec::new(),
    }
}

/// The plain-expression arguments of a function call (wildcards skipped).
fn function_args(func: &Function) -> Vec<&Expr> {
    let FunctionArguments::List(list) = &func.args else {
        return Vec::new();
    };
    list.args
        .iter()
        .filter_map(|arg| match arg {
            FunctionArg::Unnamed(FunctionArgExpr::Expr(e))
            | FunctionArg::Named {
                arg: FunctionArgExpr::Expr(e),
                ..
            } => Some(e),
            _ => None,
        })
        .collect()
}

/// Types a subquery nested in `scope` — its relations visible as the enclosing
/// scope for correlated references — appending its parameters to `params`.
fn subquery_columns(
    query: &str,
    subquery: &Query,
    scope: &Scope,
    projection: Projection,
    params: &mut Vec<Param>,
) -> Result<Vec<ResultCol>, Error> {
    query::query_columns(query, subquery, &scope.child_env(projection), params)
}

/// The one column a scalar or `IN` subquery must project, appending the
/// subquery's parameters to `params`.
fn single_column(
    query: &str,
    subquery: &Query,
    scope: &Scope,
    what: &str,
    params: &mut Vec<Param>,
) -> Result<ResultCol, Error> {
    let mut columns = subquery_columns(query, subquery, scope, Projection::Positional, params)?;
    if columns.len() != 1 {
        return Err(Error::Resolve(format!(
            "query `{query}`: {what} must project exactly one column, found {}",
            columns.len()
        )));
    }
    Ok(columns.remove(0))
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

/// Types and names a parameter from the operand opposite a placeholder,
/// appending the operand's own subquery parameters to `out`.
fn typed_param(
    query: &str,
    typed: &Expr,
    scope: &Scope,
    out: &mut Vec<Param>,
) -> Result<Param, Error> {
    let resolved = infer(query, typed, scope, out)?;
    Ok(Param {
        name: column_name(typed).unwrap_or_else(|| "param".to_string()),
        ty: resolved.ty,
        nullable: resolved.nullable,
    })
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
fn function(
    query: &str,
    func: &Function,
    scope: &Scope,
    out: &mut Vec<Param>,
) -> Result<Resolved, Error> {
    let name = last_ident(&func.name).to_ascii_lowercase();
    let args = arg_types(query, func, scope, out)?;
    if let Some(over) = &func.over {
        check_window(query, over, scope, out)?;
        if let Some(typed) = window_function(query, &name, &args, over)? {
            return Ok(typed);
        }
    }
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

/// Resolves a window: `OVER name` (and `OVER (name …)`) must name a `WINDOW`
/// of the same `SELECT`, and an inline specification is checked as
/// [`check_window_spec`] describes.
fn check_window(
    query: &str,
    over: &WindowType,
    scope: &Scope,
    out: &mut Vec<Param>,
) -> Result<(), Error> {
    match over {
        WindowType::NamedWindow(name) => scope.window(query, &name.value),
        WindowType::WindowSpec(spec) => {
            if let Some(base) = &spec.window_name {
                scope.window(query, &base.value)?;
            }
            check_window_spec(query, spec, scope, out)
        }
    }
}

/// Resolves every `PARTITION BY`/`ORDER BY` expression of a window, so an
/// unknown or ambiguous column is reported wherever it sits (`lower(nope)`).
/// An expression autosql cannot type (a literal, an unsupported form) is
/// accepted: the function's type does not depend on it.
pub(crate) fn check_window_spec(
    query: &str,
    spec: &WindowSpec,
    scope: &Scope,
    out: &mut Vec<Param>,
) -> Result<(), Error> {
    let order_by = spec.order_by.iter().map(|o| &o.expr);
    for expr in spec.partition_by.iter().chain(order_by) {
        let mut inner = Vec::new();
        match infer(query, expr, scope, &mut inner) {
            Ok(_) => out.extend(inner),
            Err(Error::Unsupported { .. }) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// The type of a window-only function, `None` for one (an aggregate such as
/// `SUM(x) OVER (…)`) that types the same as its plain form.
///
/// - Ranking (`row_number`, `rank`, `dense_rank`, `ntile`, `cume_dist`,
///   `percent_rank`) is a non-null `number`.
/// - `lag`/`lead` take the first argument's type and are nullable — the offset
///   row can be missing — unless a non-null default (third argument) covers the
///   missing row and the value itself is non-null.
/// - `first_value`/`last_value` take the first argument's type and nullability
///   under an inline window with the default frame, which always holds the
///   current row. An explicit frame (`ROWS BETWEEN 1 FOLLOWING AND …`) or a
///   named window can be empty, so there they are nullable.
/// - `nth_value` takes the first argument's type and is nullable (the frame can
///   have fewer than `n` rows).
fn window_function(
    query: &str,
    name: &str,
    args: &[Option<Resolved>],
    over: &WindowType,
) -> Result<Option<Resolved>, Error> {
    let first = || {
        args.first().copied().flatten().ok_or_else(|| {
            unsupported(
                query,
                &format!("window function `{name}` needs a value argument"),
            )
        })
    };
    Ok(Some(match name {
        "row_number" | "rank" | "dense_rank" | "ntile" | "cume_dist" | "percent_rank" => {
            prim(Prim::Number, false)
        }
        "lag" | "lead" => {
            let value = first()?;
            let defaulted = args.get(2).copied().flatten().is_some_and(|d| !d.nullable);
            Resolved {
                ty: value.ty,
                nullable: value.nullable || !defaulted,
            }
        }
        "first_value" | "last_value" => {
            let value = first()?;
            let default_frame = matches!(
                over,
                WindowType::WindowSpec(spec)
                    if spec.window_name.is_none() && spec.window_frame.is_none()
            );
            Resolved {
                nullable: value.nullable || !default_frame,
                ..value
            }
        }
        "nth_value" => Resolved {
            nullable: true,
            ..first()?
        },
        _ => return Ok(None),
    }))
}

/// Types each function argument; `None` for a `*` / `table.*` wildcard.
fn arg_types(
    query: &str,
    func: &Function,
    scope: &Scope,
    params: &mut Vec<Param>,
) -> Result<Vec<Option<Resolved>>, Error> {
    let FunctionArguments::List(list) = &func.args else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for arg in &list.args {
        let arg_expr = match arg {
            FunctionArg::Unnamed(e) | FunctionArg::Named { arg: e, .. } => e,
        };
        match arg_expr {
            FunctionArgExpr::Expr(e) => out.push(Some(infer(query, e, scope, params)?)),
            FunctionArgExpr::Wildcard | FunctionArgExpr::QualifiedWildcard(_) => out.push(None),
        }
    }
    Ok(out)
}

/// The nullability of an operand, appending its subquery parameters to `out`.
/// An operand autosql cannot type (a nested `NULL` or placeholder) counts as
/// non-null — a best effort that never fails the whole expression over it — but
/// an unknown or ambiguous column is still an error.
fn nullable(query: &str, expr: &Expr, scope: &Scope, out: &mut Vec<Param>) -> Result<bool, Error> {
    let mut inner = Vec::new();
    match infer(query, expr, scope, &mut inner) {
        Ok(r) => {
            out.extend(inner);
            Ok(r.nullable)
        }
        Err(Error::Unsupported { .. }) => {
            subquery_params(query, expr, scope, out)?;
            Ok(false)
        }
        Err(e) => Err(e),
    }
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
