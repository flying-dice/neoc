//! Single-query parsing and type inference.
//!
//! Each query is one SQL statement from a `dml/{name}.sql` file. The file stem
//! gives the neoc function name; the statement *kind* decides the return shape
//! (a `SELECT` returns rows, anything else returns rows-affected). For each
//! query we parse the SQL, resolve its columns and parameters against the
//! [`Catalog`] (see [`Scope`] for join-aware resolution and [`crate::expr`] for
//! expression typing), and record an [`AnalyzedQuery`] the emitter turns into a
//! typed neoc function.

use std::ops::ControlFlow;

use sqlparser::ast::{
    AssignmentTarget, Expr, FromTable, Insert, NamedWindowExpr, Query, Select, SelectItem, SetExpr,
    SetOperator, Statement, visit_expressions,
};

use crate::catalog::{Catalog, Column, Table, last_ident};
use crate::expr;
use crate::naming;
use crate::scope::{self, Env, Projection, Scope};
use crate::{ColType, Error, Prim, Variant};

/// How a query returns: every matching row, or rows-affected. The kind is
/// inferred from the statement — a `SELECT` returns rows, everything else
/// (`INSERT`/`UPDATE`/`DELETE`) returns the affected count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A row-returning query → `Result<[Row], string>`.
    Many,
    /// A non-returning statement → `Result<number, string>` (rows affected).
    Exec,
}

impl Kind {
    /// Whether the query decodes rows (and so needs a result struct + decoder).
    pub fn returns_rows(self) -> bool {
        matches!(self, Kind::Many)
    }
}

/// One result column of a row-returning query.
#[derive(Debug, Clone)]
pub(crate) struct ResultCol {
    /// The neoc struct field name (the column name, its alias, or a synthesised
    /// name for an expression).
    pub field: String,
    /// The neoc-facing type.
    pub ty: ColType,
    /// `true` when the column can be NULL, so the field is `Option<T>`.
    pub nullable: bool,
    /// `true` for a literal `NULL` projected by an `EXISTS` subquery
    /// (`SELECT NULL`): `ty` is a placeholder, and a set operation takes the
    /// other side's type for it.
    pub untyped: bool,
}

/// One bound parameter, in placeholder order.
#[derive(Debug, Clone)]
pub(crate) struct Param {
    /// The neoc function-parameter name — the column the placeholder is bound
    /// against, or a generic `param` when the bind site carries no column name.
    pub name: String,
    /// The neoc-facing type.
    pub ty: ColType,
    /// `true` when the bound column is nullable, so the parameter is `Option<T>`.
    pub nullable: bool,
}

/// The product of inferring one statement: its parameters and result columns
/// (empty for a non-returning `:exec`).
type Inferred = (Vec<Param>, Vec<ResultCol>);

/// A fully analysed query, ready to emit.
#[derive(Debug, Clone)]
pub(crate) struct AnalyzedQuery {
    /// The query name — the `dml` file stem, already a neoc identifier base.
    /// Drives the function name, the row-struct name, and the output file name.
    pub name: String,
    /// How the query returns (inferred from the statement kind).
    pub kind: Kind,
    /// The SQL text, whitespace-normalised, emitted verbatim into the call.
    pub sql: String,
    /// Bound parameters, in placeholder order.
    pub params: Vec<Param>,
    /// Result columns (empty for a non-returning statement).
    pub result: Vec<ResultCol>,
}

impl AnalyzedQuery {
    /// Parses and types one query's SQL against `catalog`. `name` is the
    /// `dml/{name}.sql` file stem; the statement kind decides the return shape.
    pub fn analyze(
        name: &str,
        sql: &str,
        catalog: &Catalog,
        dialect: crate::Dialect,
    ) -> Result<AnalyzedQuery, Error> {
        let sql = sql.trim();
        let mut statements = dialect.parse(sql)?;
        if statements.len() != 1 {
            return Err(Error::Unsupported {
                name: name.to_string(),
                reason: format!(
                    "a `dml` file holds exactly one query, found {} statements",
                    statements.len()
                ),
            });
        }
        let statement = statements.pop().expect("length checked above");
        let kind = if matches!(statement, Statement::Query(_)) {
            Kind::Many
        } else {
            Kind::Exec
        };
        let (mut params, mut result) = infer(name, &statement, catalog)?;
        if kind.returns_rows() && result.is_empty() {
            return Err(Error::Unsupported {
                name: name.to_string(),
                reason: "a SELECT must project at least one column".to_string(),
            });
        }
        // Every bind placeholder must have produced exactly one parameter. A
        // mismatch means a placeholder slipped through a clause we don't bind
        // from (so the generated function would be short a value), or an INSERT
        // lists more columns than its VALUES supplies — both bind the wrong
        // count at runtime, so fail loudly here instead.
        let placeholders = placeholder_count(&statement);
        if params.len() != placeholders {
            return Err(Error::PlaceholderMismatch {
                name: name.to_string(),
                placeholders,
                params: params.len(),
            });
        }
        dedup_names(params.iter_mut().map(|p| &mut p.name));
        // Only the generated struct needs unique field names. A CTE or derived
        // table keeps its duplicates, so naming one there is ambiguous, as in SQL.
        dedup_names(result.iter_mut().map(|c| &mut c.field));
        Ok(AnalyzedQuery {
            name: name.to_string(),
            kind,
            sql: normalize_sql(sql),
            params,
            result,
        })
    }

    /// The neoc function name — the sanitised `snake_case` query name.
    pub fn fn_name(&self) -> String {
        naming::sanitize_ident(&naming::snake(&self.name))
    }

    /// The row-struct name for a row-returning query: `<Pascal>Row`.
    pub fn row_name(&self) -> String {
        format!("{}Row", naming::pascal(&self.name))
    }
}

/// Dispatches inference by statement kind.
fn infer(name: &str, statement: &Statement, catalog: &Catalog) -> Result<Inferred, Error> {
    match statement {
        Statement::Query(query) => infer_select(name, query, catalog),
        Statement::Insert(insert) => infer_insert(name, insert, catalog),
        Statement::Update {
            table,
            assignments,
            selection,
            returning,
            ..
        } => {
            let mut params = Vec::new();
            let scope = Scope::build(
                name,
                std::slice::from_ref(table),
                &Env::root(catalog),
                &mut params,
            )?;
            for assignment in assignments {
                if let AssignmentTarget::ColumnName(col) = &assignment.target {
                    let col = last_ident(col);
                    let resolved = scope.unqualified(name, &col)?;
                    expr::set_params(&col, resolved, &assignment.value, &mut params);
                }
            }
            if let Some(where_clause) = selection {
                expr::params(name, where_clause, &scope, &mut params)?;
            }
            let result = returning_columns(name, &scope, returning.as_deref(), &mut params)?;
            Ok((params, result))
        }
        Statement::Delete(delete) => {
            let from = match &delete.from {
                FromTable::WithFromKeyword(tables) | FromTable::WithoutKeyword(tables) => tables,
            };
            let mut params = Vec::new();
            let scope = Scope::build(name, from, &Env::root(catalog), &mut params)?;
            if let Some(where_clause) = &delete.selection {
                expr::params(name, where_clause, &scope, &mut params)?;
            }
            let result = returning_columns(name, &scope, delete.returning.as_deref(), &mut params)?;
            Ok((params, result))
        }
        _ => Err(Error::Unsupported {
            name: name.to_string(),
            reason: "only SELECT, INSERT, UPDATE, and DELETE are supported".to_string(),
        }),
    }
}

/// Infers a `SELECT` statement's result columns and parameters.
fn infer_select(name: &str, query: &Query, catalog: &Catalog) -> Result<Inferred, Error> {
    let mut params = Vec::new();
    let result = query_columns(name, query, &Env::root(catalog), &mut params)?;
    Ok((params, result))
}

/// Types a whole query — its `WITH` clause, body, and `LIMIT`/`OFFSET` — and
/// appends its bind parameters to `params` in textual order. Used for a
/// top-level `SELECT`, a CTE body, a derived table, and a nested subquery
/// alike; `env` carries the visible CTEs and any enclosing scopes.
///
/// `WITH RECURSIVE` is refused: a recursive CTE's column types come from a
/// fixpoint over its anchor and recursive terms, which autosql does not model.
pub(crate) fn query_columns(
    name: &str,
    query: &Query,
    env: &Env,
    params: &mut Vec<Param>,
) -> Result<Vec<ResultCol>, Error> {
    let mut env = env.clone();
    if let Some(with) = &query.with {
        if with.recursive {
            return Err(Error::Unsupported {
                name: name.to_string(),
                reason: "WITH RECURSIVE is not supported".to_string(),
            });
        }
        for cte in &with.cte_tables {
            let columns = query_columns(name, &cte.query, &env.named_columns(), params)?;
            let mut table = result_table(&cte.alias.name.value, &columns);
            scope::rename_columns(name, &mut table, &cte.alias.columns)?;
            env = env.with_cte(table);
        }
    }
    let columns = set_expr_columns(name, &query.body, &env, params)?;
    limit_params(query, params);
    Ok(columns)
}

/// A relation holding a query's result columns, for a CTE or derived table to
/// expose to the rest of the query. A nullable result column is a nullable
/// relation column.
pub(crate) fn result_table(name: &str, columns: &[ResultCol]) -> Table {
    Table {
        name: name.to_string(),
        columns: columns
            .iter()
            .map(|c| Column {
                name: c.field.clone(),
                ty: c.ty,
                not_null: !c.nullable,
            })
            .collect(),
    }
}

/// Types one query body: a `SELECT`, a parenthesised query, or a set operation
/// of two bodies.
fn set_expr_columns(
    name: &str,
    body: &SetExpr,
    env: &Env,
    params: &mut Vec<Param>,
) -> Result<Vec<ResultCol>, Error> {
    match body {
        SetExpr::Select(select) => select_columns(name, select, env, params),
        SetExpr::Query(query) => query_columns(name, query, env, params),
        SetExpr::SetOperation {
            op, left, right, ..
        } => {
            let left = set_expr_columns(name, left, env, params)?;
            let right = set_expr_columns(name, right, env, params)?;
            combine_set_columns(name, *op, left, &right)
        }
        _ => Err(Error::Unsupported {
            name: name.to_string(),
            reason: "a query body must be a SELECT or a UNION/INTERSECT/EXCEPT of SELECTs \
                     (no VALUES or TABLE)"
                .to_string(),
        }),
    }
}

/// Merges the two sides of a set operation into one result shape.
///
/// Both sides must project the same number of columns, of the same neoc type
/// position by position — the database resolves set-operation types even where
/// the result is never read (`EXISTS`). An untyped side (`SELECT NULL`) takes
/// the other side's type; two untyped sides resolve to `string`, as
/// Postgres resolves `NULL ∘ NULL` to text. Field names come from
/// the left side, as in SQL. An `Int` column meeting a `Real` one binds as
/// `Real`, which holds either. A result column is nullable when a row of it can
/// be NULL: for `UNION` if either side can supply a NULL, for `EXCEPT` if the
/// left can (the right only removes rows), and for `INTERSECT` only if both can
/// (a row survives only if it appears on both sides).
fn combine_set_columns(
    name: &str,
    op: SetOperator,
    left: Vec<ResultCol>,
    right: &[ResultCol],
) -> Result<Vec<ResultCol>, Error> {
    let label = match op {
        SetOperator::Union => "UNION",
        SetOperator::Except => "EXCEPT",
        SetOperator::Intersect => "INTERSECT",
    };
    if left.len() != right.len() {
        return Err(Error::Resolve(format!(
            "query `{name}`: {label} sides project {} and {} column(s); both must project the same number",
            left.len(),
            right.len()
        )));
    }
    let mut merged = Vec::with_capacity(left.len());
    for (index, (l, r)) in left.into_iter().zip(right).enumerate() {
        if !l.untyped && !r.untyped && l.ty.prim != r.ty.prim {
            return Err(Error::Resolve(format!(
                "query `{name}`: {label} column {} is `{}` on the left but `{}` on the right",
                index + 1,
                l.ty.prim.neoc(),
                r.ty.prim.neoc()
            )));
        }
        let nullable = match op {
            SetOperator::Union => l.nullable || r.nullable,
            SetOperator::Except => l.nullable,
            SetOperator::Intersect => l.nullable && r.nullable,
        };
        let ty = if l.untyped && r.untyped {
            ColType::of(Prim::String)
        } else if l.untyped {
            r.ty
        } else if r.untyped || l.ty == r.ty || l.ty.prim != Prim::Number {
            l.ty
        } else {
            ColType {
                prim: l.ty.prim,
                variant: Variant::Real,
            }
        };
        merged.push(ResultCol {
            ty,
            nullable,
            untyped: false,
            ..l
        });
    }
    Ok(merged)
}

/// Types a plain `SELECT` and collects its parameters in textual order:
/// subqueries in the projection, then `FROM` (derived tables and `JOIN … ON`),
/// the filter clauses, and the `WINDOW` definitions.
fn select_columns(
    name: &str,
    select: &Select,
    env: &Env,
    params: &mut Vec<Param>,
) -> Result<Vec<ResultCol>, Error> {
    let mut from_params = Vec::new();
    let scope =
        Scope::build(name, &select.from, env, &mut from_params)?.with_windows(&select.named_window);
    let result = project(name, &scope, &select.projection, params)?;
    params.extend(from_params);
    select_filter_params(name, select, &scope, params)?;
    for window in &select.named_window {
        match &window.1 {
            NamedWindowExpr::NamedWindow(base) => scope.window(name, &base.value)?,
            NamedWindowExpr::WindowSpec(spec) => {
                expr::check_window_spec(name, spec, &scope, params)?;
            }
        }
    }
    Ok(result)
}

/// Collects bind parameters from a `SELECT`'s filter clauses — `WHERE`, then
/// `HAVING` — in textual placeholder order, the order the driver binds
/// `?`/`$n` positionally. `JOIN … ON` binds with `FROM` ([`Scope::build`]).
/// The projection is not a source here: a standalone `SELECT` rejects a
/// projection placeholder, and an `INSERT … SELECT` collects its value binds
/// separately (ahead of these).
fn select_filter_params(
    name: &str,
    select: &Select,
    scope: &Scope,
    out: &mut Vec<Param>,
) -> Result<(), Error> {
    if let Some(where_clause) = &select.selection {
        expr::params(name, where_clause, scope, out)?;
    }
    if let Some(having) = &select.having {
        expr::params(name, having, scope, out)?;
    }
    Ok(())
}

/// Collects the `LIMIT` / `OFFSET` placeholders of a query, which bind last.
fn limit_params(query: &Query, out: &mut Vec<Param>) {
    if is_placeholder_expr(query.limit.as_ref()) {
        out.push(number_param("limit"));
    }
    if let Some(offset) = &query.offset {
        if expr::is_placeholder(&offset.value) {
            out.push(number_param("offset"));
        }
    }
}

/// Infers an `INSERT`'s parameters and any `RETURNING` row. `INSERT … VALUES`
/// binds positionally against the column list; `INSERT … SELECT` binds through
/// the SELECT's placeholders instead (see [`infer_insert_select`]).
fn infer_insert(name: &str, insert: &Insert, catalog: &Catalog) -> Result<Inferred, Error> {
    let table_name = last_ident(&insert.table_name);
    let table = catalog
        .table(&table_name)
        .ok_or_else(|| Error::Resolve(format!("query `{name}`: unknown table `{table_name}`")))?;
    if insert.columns.is_empty() {
        return Err(Error::Unsupported {
            name: name.to_string(),
            reason: "INSERT must list its columns explicitly (`INSERT INTO t (a, b) …`)"
                .to_string(),
        });
    }
    // A query source other than `VALUES` (a SELECT — or an unsupported set
    // operation) supplies its values through the query's bind sites, not the
    // column list. Deriving them from the columns misnames a filter `?` (e.g. a
    // `WHERE`) after the column it does not bind.
    if let Some(source) = &insert.source {
        if !matches!(source.body.as_ref(), SetExpr::Values(_)) {
            return infer_insert_select(name, insert, source, table, catalog);
        }
    }
    // `INSERT … VALUES`: the placeholders match the column list positionally.
    let mut params = insert
        .columns
        .iter()
        .map(|col| param_for(name, table, &col.value))
        .collect::<Result<Vec<_>, _>>()?;
    let scope = Scope::single(catalog, table);
    let result = returning_columns(name, &scope, insert.returning.as_deref(), &mut params)?;
    Ok((params, result))
}

/// Infers the parameters of an `INSERT … SELECT` from the SELECT's bind sites,
/// in textual order:
///
/// 1. A placeholder in a projection slot is a *value* bind — it supplies the
///    target column at the same position. It is typed from that column (a bare
///    `?` carries no type of its own) but named generically: the bind is the
///    SELECT placeholder, not the destination column.
/// 2. A placeholder in `WHERE`/`JOIN … ON`/`HAVING`/`LIMIT`/`OFFSET` is a filter
///    bind, named and typed from its own clause exactly as a standalone SELECT.
///
/// A non-placeholder projection item (a copied column or expression) binds
/// nothing. `RETURNING` still describes the INSERT target table.
fn infer_insert_select(
    name: &str,
    insert: &Insert,
    source: &Query,
    table: &Table,
    catalog: &Catalog,
) -> Result<Inferred, Error> {
    let SetExpr::Select(select) = source.body.as_ref() else {
        return Err(Error::Unsupported {
            name: name.to_string(),
            reason: "only a plain SELECT may feed an INSERT (no set operations or CTEs)"
                .to_string(),
        });
    };

    check_projection_against_columns(name, select, insert)?;

    let mut params = Vec::new();
    // 1. Projection value binds, typed from the target column they feed.
    for (item, col) in select.projection.iter().zip(&insert.columns) {
        if projection_is_placeholder(item) {
            let target = param_for(name, table, &col.value)?;
            params.push(Param {
                name: "param".to_string(),
                ty: target.ty,
                nullable: target.nullable,
            });
        }
    }
    // 2. Filter binds, resolved against the SELECT's own FROM scope. A FROM-less
    //    SELECT (`SELECT ?, ?`) has none.
    if !select.from.is_empty() {
        let mut from_params = Vec::new();
        let scope = Scope::build(name, &select.from, &Env::root(catalog), &mut from_params)?;
        params.extend(from_params);
        select_filter_params(name, select, &scope, &mut params)?;
        limit_params(source, &mut params);
    }

    let scope = Scope::single(catalog, table);
    let result = returning_columns(name, &scope, insert.returning.as_deref(), &mut params)?;
    Ok((params, result))
}

/// Guards an `INSERT … SELECT`'s projection so projection value binds line up
/// with the target columns. A `*` expands to an unknown arity, so it may not sit
/// alongside a projection bind; an explicit projection must match the column
/// list one-for-one. Filter binds (`WHERE`/…) are unaffected — they bind no
/// column position.
fn check_projection_against_columns(
    name: &str,
    select: &Select,
    insert: &Insert,
) -> Result<(), Error> {
    if select.projection.iter().any(is_wildcard) {
        if select.projection.iter().any(projection_is_placeholder) {
            return Err(Error::Unsupported {
                name: name.to_string(),
                reason: "INSERT … SELECT cannot mix `*` with a bind placeholder in its projection"
                    .to_string(),
            });
        }
    } else if select.projection.len() != insert.columns.len() {
        return Err(Error::Unsupported {
            name: name.to_string(),
            reason: format!(
                "INSERT lists {} column(s) but its SELECT projects {}",
                insert.columns.len(),
                select.projection.len()
            ),
        });
    }
    Ok(())
}

/// Whether a projection item is a bare bind placeholder (`?`) or an aliased one
/// (`? AS x`) — a value bind in an `INSERT … SELECT`. A placeholder nested in a
/// larger expression (`? + 1`) is not one: it falls through to the
/// whole-statement count guard as an unbindable placeholder.
fn projection_is_placeholder(item: &SelectItem) -> bool {
    matches!(
        item,
        SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. }
            if expr::is_placeholder(e)
    )
}

/// Whether a projection item is a `*` or `table.*` wildcard.
fn is_wildcard(item: &SelectItem) -> bool {
    matches!(
        item,
        SelectItem::Wildcard(_) | SelectItem::QualifiedWildcard(..)
    )
}

/// Turns a projection (a `SELECT` list or a `RETURNING` list) into result
/// columns, expanding wildcards and typing expressions, and appends its
/// subqueries' parameters to `params`. Field names are kept as written —
/// duplicates included — for a CTE or derived table to expose.
fn project(
    name: &str,
    scope: &Scope,
    items: &[SelectItem],
    params: &mut Vec<Param>,
) -> Result<Vec<ResultCol>, Error> {
    let anonymous = scope.projection() != Projection::Named;
    let mut result = Vec::new();
    for item in items {
        match item {
            SelectItem::Wildcard(_) => {
                for (field, resolved) in scope.all_columns() {
                    result.push(ResultCol {
                        field,
                        ty: resolved.ty,
                        nullable: resolved.nullable,
                        untyped: false,
                    });
                }
            }
            SelectItem::QualifiedWildcard(object_name, _) => {
                let alias = last_ident(object_name);
                for (field, resolved) in scope.table_columns(name, &alias)? {
                    result.push(ResultCol {
                        field,
                        ty: resolved.ty,
                        nullable: resolved.nullable,
                        untyped: false,
                    });
                }
            }
            SelectItem::UnnamedExpr(e) => {
                let field = field_name(name, e, anonymous)?;
                result.push(projected(name, field, e, scope, params)?);
            }
            SelectItem::ExprWithAlias { expr: e, alias } => {
                result.push(projected(name, alias.value.clone(), e, scope, params)?);
            }
        }
    }
    Ok(result)
}

/// Types one projection item into the result column `field`. An `EXISTS`
/// subquery's projection is never read, so there a literal `NULL` is accepted
/// untyped; any other item autosql cannot type is refused, as everywhere else.
fn projected(
    name: &str,
    field: String,
    e: &Expr,
    scope: &Scope,
    params: &mut Vec<Param>,
) -> Result<ResultCol, Error> {
    if scope.projection() == Projection::Unread && expr::is_null_literal(e) {
        return Ok(ResultCol {
            field,
            ty: ColType::of(Prim::String),
            nullable: true,
            untyped: true,
        });
    }
    let resolved = expr::infer(name, e, scope, params)?;
    Ok(ResultCol {
        field,
        ty: resolved.ty,
        nullable: resolved.nullable,
        untyped: false,
    })
}

/// Result columns for a `RETURNING` clause (empty when there is none),
/// appending its subqueries' parameters to `params`.
fn returning_columns(
    name: &str,
    scope: &Scope,
    returning: Option<&[SelectItem]>,
    params: &mut Vec<Param>,
) -> Result<Vec<ResultCol>, Error> {
    match returning {
        Some(items) => project(name, scope, items, params),
        None => Ok(Vec::new()),
    }
}

/// The struct field name for an unaliased projection: the column name, or a
/// function's name (`count(*)` → `count`). Any other expression must be
/// aliased, unless its column name is never read (`anonymous`).
fn field_name(name: &str, expr: &Expr, anonymous: bool) -> Result<String, Error> {
    if let Some(col) = expr::column_name(expr) {
        return Ok(col);
    }
    match expr {
        Expr::Function(func) => Ok(last_ident(&func.name).to_ascii_lowercase()),
        Expr::Nested(inner) => field_name(name, inner, anonymous),
        _ if anonymous => Ok("column".to_string()),
        _ => Err(Error::Unsupported {
            name: name.to_string(),
            reason: "this SELECT item needs an alias (`expression AS name`)".to_string(),
        }),
    }
}

/// Builds a [`Param`] for a column by name, erroring if the table lacks it.
fn param_for(query: &str, table: &Table, col_name: &str) -> Result<Param, Error> {
    let col = table.column(col_name).ok_or_else(|| {
        Error::Resolve(format!(
            "query `{query}`: column `{col_name}` not found on table `{}`",
            table.name
        ))
    })?;
    Ok(Param {
        name: col_name.to_string(),
        ty: col.ty,
        nullable: !col.not_null,
    })
}

/// A non-null `number` parameter (for `LIMIT` / `OFFSET`).
fn number_param(name: &str) -> Param {
    Param {
        name: name.to_string(),
        ty: ColType::of(crate::Prim::Number),
        nullable: false,
    }
}

/// Whether an optional expression is a bind placeholder.
fn is_placeholder_expr(expr: Option<&Expr>) -> bool {
    expr.is_some_and(expr::is_placeholder)
}

/// Counts every bind placeholder (`?`, `$n`, …) across the whole parsed
/// statement — all clauses, subqueries included. The yardstick the inferred
/// parameter count must match.
///
/// Uses `sqlparser`'s derived expression visitor rather than a hand-rolled
/// walk: completeness is the whole point of the guard, so leaning on the
/// parser's own exhaustive traversal keeps the count correct as the AST grows,
/// instead of risking a missed variant that would silently undercount.
fn placeholder_count(statement: &Statement) -> usize {
    let mut count = 0;
    let _: ControlFlow<()> = visit_expressions(statement, |expr| {
        if expr::is_placeholder(expr) {
            count += 1;
        }
        ControlFlow::Continue(())
    });
    count
}

/// Renames duplicate identifiers to `name`, `name_2`, `name_3`, … so a
/// projection or parameter list never declares the same neoc name twice.
fn dedup_names<'a>(names: impl Iterator<Item = &'a mut String>) {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for name in names {
        let count = seen.entry(name.clone()).or_insert(0);
        *count += 1;
        if *count > 1 {
            *name = format!("{name}_{count}");
        }
    }
}

/// Collapses runs of whitespace to single spaces and trims a trailing `;`, so
/// the SQL embeds cleanly as a single-line neoc string literal.
fn normalize_sql(sql: &str) -> String {
    let collapsed = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.trim_end_matches(';').trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Dialect, Prim};

    fn catalog() -> Catalog {
        Catalog::from_schema(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT); \
             CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);",
            Dialect::Sqlite,
        )
        .unwrap()
    }

    fn one(name: &str, sql: &str) -> AnalyzedQuery {
        AnalyzedQuery::analyze(name, sql, &catalog(), Dialect::Sqlite).unwrap()
    }

    #[test]
    fn select_infers_columns_and_param() {
        let q = one(
            "user_by_id",
            "SELECT id, name, email FROM users WHERE id = ?;",
        );
        assert_eq!(q.kind, Kind::Many);
        assert_eq!(q.result.len(), 3);
        assert!(!q.result[1].nullable, "name is NOT NULL");
        assert!(q.result[2].nullable, "email is nullable");
        assert_eq!(q.params.len(), 1);
        assert_eq!(q.params[0].name, "id");
        assert_eq!(q.params[0].ty.prim, Prim::Number);
    }

    #[test]
    fn select_kind_is_many_non_select_is_exec() {
        assert_eq!(one("a", "SELECT id FROM users;").kind, Kind::Many);
        assert_eq!(
            one("b", "INSERT INTO users (name) VALUES (?);").kind,
            Kind::Exec
        );
        assert_eq!(one("c", "DELETE FROM users WHERE id = ?;").kind, Kind::Exec);
    }

    #[test]
    fn left_join_makes_right_side_nullable() {
        // posts.title is NOT NULL, but a LEFT JOIN can leave it absent.
        let q = one(
            "user_posts",
            "SELECT users.name, posts.title FROM users \
             LEFT JOIN posts ON posts.user_id = users.id;",
        );
        assert_eq!(q.result.len(), 2);
        assert!(!q.result[0].nullable, "users.name stays non-null");
        assert!(
            q.result[1].nullable,
            "posts.title is nullable under LEFT JOIN"
        );
    }

    #[test]
    fn aggregate_and_alias_typing() {
        let q = one(
            "stats",
            "SELECT count(*) AS total, max(id) AS newest FROM users;",
        );
        assert_eq!(q.result[0].field, "total");
        assert_eq!(q.result[0].ty.prim, Prim::Number);
        assert!(!q.result[0].nullable, "count is never null");
        assert_eq!(q.result[1].field, "newest");
        assert!(q.result[1].nullable, "max is null over an empty set");
    }

    #[test]
    fn unaliased_function_gets_a_name() {
        let q = one("n", "SELECT count(*) FROM users;");
        assert_eq!(q.result[0].field, "count");
    }

    #[test]
    fn ambiguous_column_is_rejected() {
        let err = AnalyzedQuery::analyze(
            "bad",
            "SELECT id FROM users JOIN posts ON posts.user_id = users.id;",
            &catalog(),
            Dialect::Sqlite,
        )
        .unwrap_err();
        assert!(
            matches!(err, Error::Resolve(_)),
            "id is ambiguous across the join"
        );
    }

    #[test]
    fn between_and_in_bind_each_placeholder() {
        let q = one(
            "p",
            "SELECT id FROM posts WHERE user_id BETWEEN ? AND ? AND id IN (?, ?);",
        );
        assert_eq!(q.params.len(), 4);
    }

    #[test]
    fn insert_returning_makes_a_row() {
        // RETURNING gives an INSERT a row shape, but the kind stays Exec — a
        // non-SELECT statement returns rows-affected.
        let q = one(
            "new_user",
            "INSERT INTO users (name, email) VALUES (?, ?) RETURNING id, name;",
        );
        assert_eq!(q.kind, Kind::Exec);
        assert_eq!(q.params.len(), 2);
        assert_eq!(q.result.len(), 2, "RETURNING gives the row shape");
        assert_eq!(q.result[0].field, "id");
    }

    #[test]
    fn two_statements_in_one_file_is_rejected() {
        let err = AnalyzedQuery::analyze(
            "two",
            "SELECT id FROM users; SELECT name FROM users;",
            &catalog(),
            Dialect::Sqlite,
        )
        .unwrap_err();
        assert!(matches!(err, Error::Unsupported { .. }));
    }

    #[test]
    fn select_with_no_columns_is_rejected() {
        // A SELECT that projects nothing parseable as a column — degenerate, but
        // exercises the empty-result guard.
        let err = AnalyzedQuery::analyze("bad", "SELECT FROM users;", &catalog(), Dialect::Sqlite);
        // Either a SQL parse error or the empty-projection guard — both are errors.
        assert!(err.is_err());
    }

    #[test]
    fn join_on_placeholder_binds_before_where() {
        // A placeholder in a JOIN … ON predicate is collected, and in textual
        // order: the ON parameter precedes the WHERE parameter.
        let q = one(
            "filtered_join",
            "SELECT users.name FROM users \
             JOIN posts ON posts.user_id = users.id AND posts.title = ? \
             WHERE users.id = ?;",
        );
        assert_eq!(q.params.len(), 2);
        assert_eq!(q.params[0].name, "title", "JOIN ON param comes first");
        assert_eq!(q.params[1].name, "id", "WHERE param follows");
    }

    #[test]
    fn insert_column_count_beyond_placeholders_is_an_error() {
        // Two columns, one VALUES placeholder: the old code emitted two params
        // for one `?`. Now it is a clear error.
        let err = AnalyzedQuery::analyze(
            "short_insert",
            "INSERT INTO users (name, email) VALUES (?);",
            &catalog(),
            Dialect::Sqlite,
        )
        .unwrap_err();
        assert!(
            matches!(
                err,
                Error::PlaceholderMismatch {
                    placeholders: 1,
                    params: 2,
                    ..
                }
            ),
            "got {err:?}"
        );
    }

    #[test]
    fn placeholder_in_projection_is_rejected_not_dropped() {
        // A placeholder in the SELECT list is bound from nowhere; rather than
        // silently generate a parameter-short function, generation fails.
        let err = AnalyzedQuery::analyze(
            "bumped",
            "SELECT id + ? AS bumped FROM users WHERE id = ?;",
            &catalog(),
            Dialect::Sqlite,
        )
        .unwrap_err();
        assert!(
            matches!(
                err,
                Error::PlaceholderMismatch {
                    placeholders: 2,
                    params: 1,
                    ..
                }
            ),
            "got {err:?}"
        );
    }

    #[test]
    fn insert_select_params_derive_from_select_not_target_columns() {
        // The values come from the SELECT, so each `?` is a SELECT bind — named
        // for the bind site (a generic `param`), not the destination column.
        // Old behaviour named them after `user_id`/`title`.
        let q = one(
            "copy_post",
            "INSERT INTO posts (user_id, title) SELECT ?, ?;",
        );
        assert_eq!(q.kind, Kind::Exec);
        assert_eq!(q.params.len(), 2);
        assert_eq!(
            q.params[0].name, "param",
            "named for the bind site, not user_id"
        );
        assert_eq!(q.params[1].name, "param_2", "deduped, not title");
        // Type still follows the column the value lands in — a bare `?` has none.
        assert_eq!(q.params[0].ty.prim, Prim::Number, "user_id is INTEGER");
        assert_eq!(q.params[1].ty.prim, Prim::String, "title is TEXT");
    }

    #[test]
    fn insert_select_filter_bind_names_its_own_column_not_the_target() {
        // The misleading case the column-derived path got wrong: a projection
        // `?` (feeding `title`) then a WHERE `?` (filtering `users.name`). The
        // second bind is `name` — its own clause — not the target column `title`.
        let q = one(
            "copy_named",
            "INSERT INTO posts (user_id, title) \
             SELECT id, ? FROM users WHERE name = ?;",
        );
        assert_eq!(q.params.len(), 2);
        assert_eq!(
            q.params[0].name, "param",
            "projection bind, textually first"
        );
        assert_eq!(
            q.params[1].name, "name",
            "WHERE bind names its own column, not the target `title`"
        );
        assert_eq!(q.params[1].ty.prim, Prim::String);
    }

    #[test]
    fn insert_select_with_only_filter_binds() {
        // No projection placeholder — a straight copy filtered by a WHERE bind.
        // The column-derived path rejected this (2 columns, 1 placeholder); the
        // SELECT-derived path infers the single WHERE param.
        let q = one(
            "copy_filtered",
            "INSERT INTO posts (user_id, title) \
             SELECT id, name FROM users WHERE id = ?;",
        );
        assert_eq!(q.params.len(), 1);
        assert_eq!(q.params[0].name, "id");
        assert_eq!(q.params[0].ty.prim, Prim::Number);
    }

    #[test]
    fn insert_select_projection_arity_must_match_columns() {
        // Two target columns, one projected value: the INSERT is malformed and
        // would bind the wrong arity — reject it before emitting.
        let err = AnalyzedQuery::analyze(
            "lopsided",
            "INSERT INTO posts (user_id, title) SELECT ?;",
            &catalog(),
            Dialect::Sqlite,
        )
        .unwrap_err();
        assert!(matches!(err, Error::Unsupported { .. }), "got {err:?}");
    }

    #[test]
    fn insert_select_set_operation_is_unsupported() {
        // A set-operation source is not a plain SELECT: reject it loudly rather
        // than fall back to the column-derived (mislabelled) param path.
        let err = AnalyzedQuery::analyze(
            "unioned",
            "INSERT INTO posts (user_id, title) \
             SELECT ?, ? UNION SELECT user_id, title FROM posts;",
            &catalog(),
            Dialect::Sqlite,
        )
        .unwrap_err();
        assert!(matches!(err, Error::Unsupported { .. }), "got {err:?}");
    }

    #[test]
    fn insert_select_wildcard_mixed_with_bind_is_unsupported() {
        // `*` expands to an unknown arity, so it cannot sit beside a projection
        // bind — the column positions no longer line up. Reject it rather than
        // emit a param against a column position we cannot pin down.
        let err = AnalyzedQuery::analyze(
            "star_mixed",
            "INSERT INTO posts (user_id, title) SELECT *, ? FROM users;",
            &catalog(),
            Dialect::Sqlite,
        )
        .unwrap_err();
        assert!(matches!(err, Error::Unsupported { .. }), "got {err:?}");
    }

    // --- query-inference breadth: CTEs, subqueries, set operations, windows ---

    fn refused(sql: &str) -> Error {
        AnalyzedQuery::analyze("q", sql, &catalog(), Dialect::Sqlite).unwrap_err()
    }

    #[test]
    fn cte_columns_type_from_its_query() {
        let q = one(
            "c",
            "WITH named AS (SELECT id, email FROM users) SELECT id, email FROM named WHERE id = ?;",
        );
        assert_eq!(q.result[0].field, "id");
        assert!(!q.result[0].nullable, "id stays non-null through the CTE");
        assert!(q.result[1].nullable, "email stays nullable through the CTE");
        assert_eq!(q.params.len(), 1);
        assert_eq!(q.params[0].name, "id");
    }

    #[test]
    fn cte_column_alias_list_renames_columns() {
        let q = one(
            "c",
            "WITH named(user_id, mail) AS (SELECT id, email FROM users) SELECT user_id, mail FROM named;",
        );
        assert_eq!(q.result[0].field, "user_id");
        assert_eq!(q.result[1].field, "mail");
    }

    #[test]
    fn cte_alias_list_arity_must_match() {
        let err = refused("WITH named(a) AS (SELECT id, email FROM users) SELECT a FROM named;");
        assert!(matches!(err, Error::Resolve(_)), "got {err:?}");
    }

    #[test]
    fn later_cte_sees_earlier_and_params_bind_in_textual_order() {
        let q = one(
            "c",
            "WITH a AS (SELECT id FROM users WHERE name = ?), \
                  b AS (SELECT id FROM a WHERE id > ?) \
             SELECT id FROM b WHERE id < ?;",
        );
        let names: Vec<&str> = q.params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["name", "id", "id_2"]);
    }

    #[test]
    fn recursive_cte_is_unsupported() {
        let err = refused(
            "WITH RECURSIVE r AS (SELECT id FROM users UNION ALL SELECT id FROM r) SELECT id FROM r;",
        );
        assert!(matches!(err, Error::Unsupported { .. }), "got {err:?}");
    }

    #[test]
    fn left_join_to_cte_nulls_the_cte_columns() {
        let q = one(
            "c",
            "WITH t AS (SELECT user_id, title FROM posts) \
             SELECT users.name, t.title FROM users LEFT JOIN t ON t.user_id = users.id;",
        );
        assert!(!q.result[0].nullable);
        assert!(q.result[1].nullable, "outer join widens a CTE column");
    }

    #[test]
    fn derived_table_exposes_its_projection() {
        let q = one(
            "d",
            "SELECT t.total FROM (SELECT count(*) AS total FROM users WHERE id > ?) AS t;",
        );
        assert_eq!(q.result[0].field, "total");
        assert!(
            !q.result[0].nullable,
            "count is non-null through the subquery"
        );
        assert_eq!(q.params.len(), 1);
    }

    #[test]
    fn derived_table_without_alias_is_unsupported() {
        let err = refused("SELECT id FROM (SELECT id FROM users);");
        assert!(matches!(err, Error::Unsupported { .. }), "got {err:?}");
    }

    #[test]
    fn scalar_subquery_is_nullable_and_correlated() {
        let q = one(
            "s",
            "SELECT users.name, (SELECT max(posts.id) FROM posts WHERE posts.user_id = users.id) AS newest \
             FROM users;",
        );
        assert!(q.result[1].nullable, "an empty scalar subquery is NULL");
        assert_eq!(q.result[1].ty.prim, Prim::Number);
    }

    #[test]
    fn scalar_subquery_must_project_one_column() {
        let err = refused("SELECT (SELECT id, name FROM users) AS x FROM users;");
        assert!(matches!(err, Error::Resolve(_)), "got {err:?}");
    }

    #[test]
    fn in_subquery_binds_its_placeholders() {
        let q = one(
            "i",
            "SELECT name FROM users WHERE id IN (SELECT user_id FROM posts WHERE title = ?) AND name = ?;",
        );
        let names: Vec<&str> = q.params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["title", "name"]);
    }

    #[test]
    fn delete_where_in_subquery_binds() {
        let q = one(
            "d",
            "DELETE FROM posts WHERE user_id IN (SELECT id FROM users WHERE name = ?);",
        );
        assert_eq!(q.params.len(), 1);
        assert_eq!(q.params[0].name, "name");
    }

    #[test]
    fn exists_and_in_subquery_are_non_null_bool_when_operands_are() {
        let q = one(
            "e",
            "SELECT EXISTS (SELECT 1 FROM posts) AS any_posts, \
             id IN (SELECT user_id FROM posts) AS has_post FROM users;",
        );
        assert_eq!(q.result[0].ty.prim, Prim::Bool);
        assert!(!q.result[0].nullable);
        assert!(!q.result[1].nullable, "id and posts.user_id are NOT NULL");
    }

    #[test]
    fn subquery_with_unknown_column_is_rejected() {
        let err = refused("SELECT id FROM users WHERE id IN (SELECT nope FROM posts);");
        assert!(matches!(err, Error::Resolve(_)), "got {err:?}");
    }

    #[test]
    fn union_takes_left_names_and_merges_nullability() {
        let q = one(
            "u",
            "SELECT id, email AS contact FROM users UNION SELECT id, title FROM posts;",
        );
        assert_eq!(q.result[1].field, "contact");
        assert!(q.result[1].nullable, "UNION is nullable if either side is");
    }

    #[test]
    fn except_keeps_left_nullability_intersect_needs_both() {
        let except = one(
            "x",
            "SELECT title FROM posts EXCEPT SELECT email FROM users;",
        );
        assert!(!except.result[0].nullable, "EXCEPT only removes rows");
        let intersect = one(
            "y",
            "SELECT email FROM users INTERSECT SELECT title FROM posts;",
        );
        assert!(
            !intersect.result[0].nullable,
            "INTERSECT is null only if both sides are"
        );
    }

    #[test]
    fn set_operation_params_bind_left_then_right_then_limit() {
        let q = one(
            "u",
            "SELECT id FROM users WHERE name = ? UNION ALL SELECT user_id FROM posts WHERE title = ? LIMIT ?;",
        );
        let names: Vec<&str> = q.params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["name", "title", "limit"]);
    }

    #[test]
    fn set_operation_arity_mismatch_is_rejected() {
        let err = refused("SELECT id, name FROM users UNION SELECT id FROM posts;");
        assert!(
            matches!(&err, Error::Resolve(m) if m.contains("2 and 1")),
            "got {err:?}"
        );
    }

    #[test]
    fn set_operation_type_mismatch_is_rejected() {
        let err = refused("SELECT id FROM users UNION SELECT title FROM posts;");
        assert!(
            matches!(&err, Error::Resolve(m) if m.contains("number") && m.contains("string")),
            "got {err:?}"
        );
    }

    #[test]
    fn ranking_window_functions_are_non_null_numbers() {
        let q = one(
            "w",
            "SELECT row_number() OVER (ORDER BY id) AS n, rank() OVER (PARTITION BY user_id ORDER BY id) AS r FROM posts;",
        );
        for col in &q.result {
            assert_eq!(col.ty.prim, Prim::Number);
            assert!(!col.nullable);
        }
    }

    #[test]
    fn lag_is_nullable_unless_defaulted() {
        let q = one(
            "w",
            "SELECT lag(title) OVER (ORDER BY id) AS a, lag(title, 1, 'none') OVER (ORDER BY id) AS b FROM posts;",
        );
        assert!(q.result[0].nullable, "no previous row is NULL");
        assert!(!q.result[1].nullable, "a non-null default covers it");
    }

    #[test]
    fn aggregate_over_window_keeps_aggregate_typing() {
        let q = one(
            "w",
            "SELECT count(*) OVER () AS c, sum(id) OVER (PARTITION BY user_id) AS s FROM posts;",
        );
        assert!(!q.result[0].nullable);
        assert!(q.result[1].nullable);
    }

    #[test]
    fn window_with_unknown_partition_column_is_rejected() {
        let err = refused("SELECT row_number() OVER (PARTITION BY nope) AS n FROM posts;");
        assert!(matches!(err, Error::Resolve(_)), "got {err:?}");
    }

    #[test]
    fn ambiguous_column_inside_a_subquery_is_rejected() {
        let err = refused(
            "SELECT name FROM users WHERE id IN \
             (SELECT id FROM users JOIN posts ON posts.user_id = users.id);",
        );
        assert!(
            matches!(&err, Error::Resolve(m) if m.contains("ambiguous")),
            "got {err:?}"
        );
    }

    #[test]
    fn derived_table_still_needs_aliased_expressions() {
        // Only EXISTS/IN/scalar subqueries ignore their column names; a derived
        // table is read by name, so an unaliased expression is refused.
        let err = refused("SELECT t.x FROM (SELECT id + 1 FROM users) AS t;");
        assert!(matches!(err, Error::Unsupported { .. }), "got {err:?}");
    }

    fn param_names(q: &AnalyzedQuery) -> Vec<&str> {
        q.params.iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn join_on_and_derived_table_params_interleave_in_textual_order() {
        let q = one(
            "j",
            "SELECT u.name FROM users u \
             JOIN posts p ON p.user_id = u.id AND p.id = ? \
             JOIN (SELECT id FROM users WHERE name = ?) d ON d.id = u.id;",
        );
        assert_eq!(param_names(&q), ["id", "name"]);
        assert_eq!(q.params[0].ty.prim, Prim::Number, "p.id binds first");
        assert_eq!(q.params[1].ty.prim, Prim::String);
    }

    #[test]
    fn insert_select_join_on_binds_before_a_later_derived_table() {
        let q = one(
            "i",
            "INSERT INTO posts (user_id, title) SELECT u.id, ? FROM users u \
             JOIN posts p ON p.title = ? \
             JOIN (SELECT id FROM users WHERE name = ?) d ON d.id = u.id;",
        );
        assert_eq!(param_names(&q), ["param", "title", "name"]);
    }

    #[test]
    fn first_and_last_value_are_nullable_under_an_explicit_frame() {
        let q = one(
            "w",
            "SELECT first_value(title) OVER (ORDER BY id) AS a, \
             first_value(title) OVER (ORDER BY id ROWS BETWEEN 1 FOLLOWING AND 2 FOLLOWING) AS f, \
             last_value(title) OVER (ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS l \
             FROM posts;",
        );
        assert!(
            !q.result[0].nullable,
            "the default frame holds the current row"
        );
        assert!(q.result[1].nullable, "a following-only frame can be empty");
        assert!(q.result[2].nullable, "a preceding-only frame can be empty");
    }

    #[test]
    fn first_value_over_a_named_window_is_nullable() {
        let q = one(
            "w",
            "SELECT first_value(title) OVER w AS f FROM posts \
             WINDOW w AS (ORDER BY id ROWS BETWEEN 1 FOLLOWING AND 1 FOLLOWING);",
        );
        assert!(q.result[0].nullable);
    }

    #[test]
    fn duplicate_derived_column_is_not_renamed_and_is_ambiguous() {
        let derived = "(SELECT u.id, p.id FROM users u JOIN posts p ON p.user_id = u.id) AS t";
        let err = refused(&format!("SELECT t.id_2 FROM {derived};"));
        assert!(
            matches!(&err, Error::Resolve(m) if m.contains("`id_2` not found")),
            "got {err:?}"
        );
        let err = refused(&format!("SELECT t.id FROM {derived};"));
        assert!(
            matches!(&err, Error::Resolve(m) if m.contains("ambiguous")),
            "got {err:?}"
        );
        let err = refused(
            "WITH t AS (SELECT u.id, p.id FROM users u JOIN posts p ON p.user_id = u.id) SELECT id FROM t;",
        );
        assert!(
            matches!(&err, Error::Resolve(m) if m.contains("ambiguous")),
            "got {err:?}"
        );
    }

    #[test]
    fn duplicate_names_are_still_deduplicated_in_the_row_struct() {
        let q = one(
            "s",
            "SELECT * FROM (SELECT u.id, p.id FROM users u JOIN posts p ON p.user_id = u.id) AS t;",
        );
        let fields: Vec<&str> = q.result.iter().map(|c| c.field.as_str()).collect();
        assert_eq!(fields, ["id", "id_2"]);
    }

    #[test]
    fn window_expressions_and_names_are_resolved() {
        let err = refused("SELECT row_number() OVER (PARTITION BY lower(nope)) AS n FROM posts;");
        assert!(
            matches!(&err, Error::Resolve(m) if m.contains("`nope`")),
            "got {err:?}"
        );
        let err = refused("SELECT rank() OVER undefined_w AS r FROM posts;");
        assert!(
            matches!(&err, Error::Resolve(m) if m.contains("no window `undefined_w`")),
            "got {err:?}"
        );
        let err = refused("SELECT rank() OVER w AS r FROM posts WINDOW w AS (ORDER BY nope);");
        assert!(
            matches!(&err, Error::Resolve(m) if m.contains("`nope`")),
            "got {err:?}"
        );
    }

    #[test]
    fn exists_accepts_an_untyped_projection() {
        let q = one(
            "e",
            "SELECT name FROM users WHERE EXISTS (SELECT NULL FROM posts WHERE posts.user_id = users.id AND posts.title = ?);",
        );
        assert_eq!(param_names(&q), ["title"]);
        let err = refused("SELECT name FROM users WHERE EXISTS (SELECT nope FROM posts);");
        assert!(
            matches!(&err, Error::Resolve(m) if m.contains("`nope`")),
            "an unknown column is still refused: {err:?}"
        );
    }

    #[test]
    fn null_operand_makes_the_expression_nullable() {
        let q = one(
            "n",
            "SELECT name || NULL AS s, id + NULL AS a, (NULL) * id AS m, \
             CASE WHEN id > 1 THEN name ELSE NULL END AS c FROM users;",
        );
        for col in &q.result {
            assert!(col.nullable, "`{}` propagates NULL", col.field);
        }
    }

    #[test]
    fn exists_set_operation_still_checks_types() {
        let err = AnalyzedQuery::analyze(
            "e",
            "SELECT name FROM users WHERE EXISTS (SELECT id FROM users UNION SELECT title FROM posts);",
            &catalog(),
            Dialect::Postgres,
        )
        .unwrap_err();
        assert!(
            matches!(&err, Error::Resolve(m) if m.contains("`number` on the left but `string`")),
            "got {err:?}"
        );
    }

    #[test]
    fn exists_untyped_side_takes_the_other_sides_type() {
        for sql in [
            "SELECT name FROM users WHERE EXISTS (SELECT NULL FROM posts UNION SELECT id FROM users);",
            "SELECT name FROM users WHERE EXISTS (SELECT title FROM posts UNION SELECT NULL FROM users);",
            "SELECT name FROM users WHERE EXISTS (SELECT NULL FROM posts UNION SELECT NULL FROM users);",
        ] {
            let q = one("e", sql);
            assert_eq!(q.result.len(), 1, "{sql}");
        }
    }

    fn refused_on_postgres(sql: &str) -> Error {
        AnalyzedQuery::analyze("q", sql, &catalog(), Dialect::Postgres).unwrap_err()
    }

    #[test]
    fn exists_refuses_untypable_items_other_than_null() {
        let err = refused_on_postgres(
            "SELECT name FROM users WHERE EXISTS \
             (SELECT POSITION('a' IN title) FROM posts UNION SELECT title FROM posts);",
        );
        assert!(matches!(err, Error::Unsupported { .. }), "got {err:?}");
        let err = refused_on_postgres(
            "SELECT name FROM users WHERE EXISTS \
             (SELECT ARRAY[1] FROM posts UNION SELECT title FROM posts);",
        );
        assert!(matches!(err, Error::Unsupported { .. }), "got {err:?}");
    }

    #[test]
    fn exists_null_set_null_resolves_to_string() {
        let err = refused_on_postgres(
            "SELECT name FROM users WHERE EXISTS (SELECT NULL FROM posts \
             INTERSECT SELECT NULL FROM users EXCEPT SELECT id FROM users);",
        );
        assert!(
            matches!(&err, Error::Resolve(m) if m.contains("`string` on the left but `number`")),
            "got {err:?}"
        );
    }

    #[test]
    fn set_operation_of_int_and_real_binds_as_real() {
        let catalog = Catalog::from_schema(
            "CREATE TABLE a (n INTEGER NOT NULL); CREATE TABLE b (r REAL NOT NULL);",
            Dialect::Sqlite,
        )
        .unwrap();
        let q = AnalyzedQuery::analyze(
            "v",
            "WITH v AS (SELECT n FROM a UNION SELECT r FROM b) SELECT n FROM v WHERE n = ?;",
            &catalog,
            Dialect::Sqlite,
        )
        .unwrap();
        assert_eq!(q.params[0].ty.variant, Variant::Real);
        assert_eq!(q.result[0].ty.variant, Variant::Real);
    }

    #[test]
    fn deeply_nested_subqueries_type_each_level_once() {
        // Typing each level twice made this 2^depth: depth 16 took ~9 s.
        let depth = 16;
        let mut sql = "SELECT max(id) FROM users WHERE name = ?".to_string();
        for _ in 0..depth {
            sql = format!("SELECT ({sql}) AS v FROM users WHERE name = ?");
        }
        let q = one("deep", &format!("{sql};"));
        assert_eq!(q.params.len(), depth + 1);
    }

    #[test]
    fn named_window_and_chained_set_operations_are_accepted() {
        let q = one(
            "w",
            "SELECT rank() OVER w AS r FROM posts WINDOW w AS (ORDER BY id);",
        );
        assert!(!q.result[0].nullable);
        let q = one(
            "u",
            "SELECT id FROM users UNION ALL SELECT user_id FROM posts \
             EXCEPT (SELECT id FROM posts);",
        );
        assert_eq!(q.result.len(), 1);
    }
}
