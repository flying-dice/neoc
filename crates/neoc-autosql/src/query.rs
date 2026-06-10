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
    AssignmentTarget, Expr, FromTable, Insert, JoinConstraint, JoinOperator, Query, Select,
    SelectItem, SetExpr, Statement, TableWithJoins, visit_expressions,
};

use crate::catalog::{Catalog, Table, last_ident};
use crate::expr;
use crate::naming;
use crate::scope::Scope;
use crate::{ColType, Error};

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

/// The product of inferring one statement: its parameters, result columns
/// (empty for a non-returning `:exec`), and the lone table it touches (if any).
type Inferred = (Vec<Param>, Vec<ResultCol>, Option<String>);

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
        let (mut params, result, _table) = infer(name, &statement, catalog)?;
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
            let scope = Scope::build(name, std::slice::from_ref(table), catalog)?;
            let mut params = Vec::new();
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
            let result = returning_columns(name, &scope, returning.as_deref())?;
            Ok((params, result, scope.sole_table()))
        }
        Statement::Delete(delete) => {
            let from = match &delete.from {
                FromTable::WithFromKeyword(tables) | FromTable::WithoutKeyword(tables) => tables,
            };
            let scope = Scope::build(name, from, catalog)?;
            let mut params = Vec::new();
            if let Some(where_clause) = &delete.selection {
                expr::params(name, where_clause, &scope, &mut params)?;
            }
            let result = returning_columns(name, &scope, delete.returning.as_deref())?;
            Ok((params, result, scope.sole_table()))
        }
        _ => Err(Error::Unsupported {
            name: name.to_string(),
            reason: "only SELECT, INSERT, UPDATE, and DELETE are supported".to_string(),
        }),
    }
}

/// Infers a `SELECT`'s result columns (projection), parameters (`WHERE` /
/// `HAVING` / `LIMIT` / `OFFSET`), and lone-table reuse hint.
fn infer_select(
    name: &str,
    query: &sqlparser::ast::Query,
    catalog: &Catalog,
) -> Result<Inferred, Error> {
    let SetExpr::Select(select) = query.body.as_ref() else {
        return Err(Error::Unsupported {
            name: name.to_string(),
            reason: "only plain SELECT queries are supported (no set operations or CTEs)"
                .to_string(),
        });
    };
    let scope = Scope::build(name, &select.from, catalog)?;
    let result = project(name, &scope, &select.projection)?;
    let mut params = Vec::new();
    select_filter_params(name, query, select, &scope, &mut params)?;
    Ok((params, result, scope.sole_table()))
}

/// Collects bind parameters from a `SELECT`'s filter clauses — `JOIN … ON`,
/// `WHERE`, `HAVING`, then `LIMIT`/`OFFSET` — in textual placeholder order, the
/// order the driver binds `?`/`$n` positionally. The projection is not a source
/// here: a standalone `SELECT` rejects a projection placeholder, and an
/// `INSERT … SELECT` collects its value binds separately (ahead of these).
fn select_filter_params(
    name: &str,
    query: &Query,
    select: &Select,
    scope: &Scope,
    out: &mut Vec<Param>,
) -> Result<(), Error> {
    join_params(name, &select.from, scope, out)?;
    if let Some(where_clause) = &select.selection {
        expr::params(name, where_clause, scope, out)?;
    }
    if let Some(having) = &select.having {
        expr::params(name, having, scope, out)?;
    }
    if is_placeholder_expr(query.limit.as_ref()) {
        out.push(number_param("limit"));
    }
    if let Some(offset) = &query.offset {
        if expr::is_placeholder(&offset.value) {
            out.push(number_param("offset"));
        }
    }
    Ok(())
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
    let params = insert
        .columns
        .iter()
        .map(|col| param_for(name, table, &col.value))
        .collect::<Result<Vec<_>, _>>()?;
    let scope = Scope::single(table);
    let result = returning_columns(name, &scope, insert.returning.as_deref())?;
    Ok((params, result, Some(table.name.clone())))
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
        let scope = Scope::build(name, &select.from, catalog)?;
        select_filter_params(name, source, select, &scope, &mut params)?;
    }

    let scope = Scope::single(table);
    let result = returning_columns(name, &scope, insert.returning.as_deref())?;
    Ok((params, result, Some(table.name.clone())))
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
/// columns, expanding wildcards and typing expressions, then de-duplicates
/// field names.
fn project(name: &str, scope: &Scope, items: &[SelectItem]) -> Result<Vec<ResultCol>, Error> {
    let mut result = Vec::new();
    for item in items {
        match item {
            SelectItem::Wildcard(_) => {
                for (field, resolved) in scope.all_columns() {
                    result.push(ResultCol {
                        field,
                        ty: resolved.ty,
                        nullable: resolved.nullable,
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
                    });
                }
            }
            SelectItem::UnnamedExpr(e) => {
                let resolved = expr::infer(name, e, scope)?;
                result.push(ResultCol {
                    field: field_name(name, e)?,
                    ty: resolved.ty,
                    nullable: resolved.nullable,
                });
            }
            SelectItem::ExprWithAlias { expr: e, alias } => {
                let resolved = expr::infer(name, e, scope)?;
                result.push(ResultCol {
                    field: alias.value.clone(),
                    ty: resolved.ty,
                    nullable: resolved.nullable,
                });
            }
        }
    }
    dedup_names(result.iter_mut().map(|c| &mut c.field));
    Ok(result)
}

/// Result columns for a `RETURNING` clause (empty when there is none).
fn returning_columns(
    name: &str,
    scope: &Scope,
    returning: Option<&[SelectItem]>,
) -> Result<Vec<ResultCol>, Error> {
    match returning {
        Some(items) => project(name, scope, items),
        None => Ok(Vec::new()),
    }
}

/// The struct field name for an unaliased projection: the column name, or a
/// function's name (`count(*)` → `count`). Any other expression must be aliased.
fn field_name(name: &str, expr: &Expr) -> Result<String, Error> {
    if let Some(col) = expr::column_name(expr) {
        return Ok(col);
    }
    match expr {
        Expr::Function(func) => Ok(last_ident(&func.name).to_ascii_lowercase()),
        Expr::Nested(inner) => field_name(name, inner),
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

/// Collects parameters from each `JOIN … ON` predicate, walked in `FROM`-clause
/// order so a placeholder in a join condition binds in its textual position
/// (ahead of the `WHERE`). `USING`/`NATURAL`/cross joins carry no predicate.
fn join_params(
    name: &str,
    from: &[TableWithJoins],
    scope: &Scope,
    out: &mut Vec<Param>,
) -> Result<(), Error> {
    for table in from {
        for join in &table.joins {
            if let Some(on) = join_on(&join.join_operator) {
                expr::params(name, on, scope, out)?;
            }
        }
    }
    Ok(())
}

/// The `ON` predicate of a join, if it has one. `USING`, `NATURAL`, and the
/// predicate-less cross joins return `None`.
fn join_on(op: &JoinOperator) -> Option<&Expr> {
    let constraint = match op {
        JoinOperator::Inner(c)
        | JoinOperator::LeftOuter(c)
        | JoinOperator::RightOuter(c)
        | JoinOperator::FullOuter(c)
        | JoinOperator::LeftSemi(c)
        | JoinOperator::RightSemi(c)
        | JoinOperator::LeftAnti(c)
        | JoinOperator::RightAnti(c)
        | JoinOperator::AsOf { constraint: c, .. } => c,
        JoinOperator::CrossJoin | JoinOperator::CrossApply | JoinOperator::OuterApply => {
            return None;
        }
    };
    match constraint {
        JoinConstraint::On(expr) => Some(expr),
        JoinConstraint::Using(_) | JoinConstraint::Natural | JoinConstraint::None => None,
    }
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
}
