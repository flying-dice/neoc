//! Single-query parsing and type inference.
//!
//! Each query is one SQL statement from a `dml/{name}.sql` file. The file stem
//! gives the neoc function name; the statement *kind* decides the return shape
//! (a `SELECT` returns rows, anything else returns rows-affected). For each
//! query we parse the SQL, resolve its columns and parameters against the
//! [`Catalog`] (see [`Scope`] for join-aware resolution and [`crate::expr`] for
//! expression typing), and record an [`AnalyzedQuery`] the emitter turns into a
//! typed neoc function.

use sqlparser::ast::{AssignmentTarget, Expr, FromTable, SelectItem, SetExpr, Statement};

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
    /// The neoc function-parameter name (derived from the column it binds).
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
    if let Some(where_clause) = &select.selection {
        expr::params(name, where_clause, &scope, &mut params)?;
    }
    if let Some(having) = &select.having {
        expr::params(name, having, &scope, &mut params)?;
    }
    if is_placeholder_expr(query.limit.as_ref()) {
        params.push(number_param("limit"));
    }
    if let Some(offset) = &query.offset {
        if expr::is_placeholder(&offset.value) {
            params.push(number_param("offset"));
        }
    }
    Ok((params, result, scope.sole_table()))
}

/// Infers an `INSERT`'s parameters from its column list (the `VALUES`
/// placeholders are assumed to match it positionally) and any `RETURNING`.
fn infer_insert(
    name: &str,
    insert: &sqlparser::ast::Insert,
    catalog: &Catalog,
) -> Result<Inferred, Error> {
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
    let params = insert
        .columns
        .iter()
        .map(|col| param_for(name, table, &col.value))
        .collect::<Result<Vec<_>, _>>()?;
    let scope = Scope::single(table);
    let result = returning_columns(name, &scope, insert.returning.as_deref())?;
    Ok((params, result, Some(table.name.clone())))
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
}
