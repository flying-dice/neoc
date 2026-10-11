//! Query resolution scope: the relations a single `SELECT`/`UPDATE`/`DELETE`
//! can see, with alias-aware column lookup and outer-join nullability.
//!
//! A `FROM` clause is one or more comma-joined relations, each possibly
//! carrying `JOIN`s. A relation is a schema table, a CTE (`WITH name AS (…)`),
//! or a derived table (`(SELECT …) AS name`); CTEs and derived tables are typed
//! from the query that defines them. Each relation enters the scope under its
//! alias (or its own name). An outer join can make a side's columns NULL even
//! when the schema marks them `NOT NULL`: `LEFT JOIN` nulls the joined
//! relation, `RIGHT JOIN` nulls the relations already in scope, `FULL JOIN`
//! nulls both.
//!
//! A subquery nested in an expression resolves against its own scope first and
//! then the enclosing scopes (a correlated reference), nearest enclosing scope
//! first. The enclosing scopes and the visible CTEs travel in an [`Env`].

use sqlparser::ast::{Ident, Join, JoinOperator, TableFactor, TableWithJoins};

use crate::catalog::{Catalog, Column, Table, last_ident};
use crate::query::{self, Param};
use crate::{ColType, Error};

/// One relation in scope, under its alias, with whether an outer join can NULL it.
#[derive(Clone)]
struct Source {
    /// The alias the query refers to it by — an `AS` alias or the relation name.
    alias: String,
    table: Table,
    /// `true` when an outer join can make this relation's columns NULL.
    nullable_by_join: bool,
}

/// A resolved column reference: its neoc type and whether it can be NULL
/// (schema nullability OR an outer join).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Resolved {
    pub ty: ColType,
    pub nullable: bool,
}

/// What a (sub)query can see beyond its own `FROM`: the schema, the CTEs
/// declared by enclosing `WITH` clauses, and the enclosing scopes' relations
/// for correlated references.
#[derive(Clone)]
pub(crate) struct Env<'a> {
    catalog: &'a Catalog,
    /// Visible CTEs, in declaration order; a later declaration shadows an earlier one.
    ctes: Vec<Table>,
    /// Enclosing scopes' relations, nearest first.
    outer: Vec<Vec<Source>>,
    /// `true` inside an `EXISTS`/`IN`/scalar subquery, whose result column names
    /// are never read — so an unaliased expression needs no alias there.
    anonymous_columns: bool,
}

impl<'a> Env<'a> {
    /// The environment of a top-level statement: the schema, no CTEs, no enclosing scope.
    pub fn root(catalog: &'a Catalog) -> Env<'a> {
        Env {
            catalog,
            ctes: Vec::new(),
            outer: Vec::new(),
            anonymous_columns: false,
        }
    }

    /// This environment for a query whose result columns are read by name — a
    /// CTE body or derived table — so unaliased expressions are refused.
    pub fn named_columns(&self) -> Env<'a> {
        Env {
            anonymous_columns: false,
            ..self.clone()
        }
    }

    /// This environment extended with a CTE visible to later CTEs and the body.
    pub fn with_cte(&self, table: Table) -> Env<'a> {
        let mut env = self.clone();
        env.ctes.push(table);
        env
    }

    /// Finds a named relation: a CTE first (it shadows a schema table), then the schema.
    fn relation(&self, name: &str) -> Option<&Table> {
        self.ctes
            .iter()
            .rev()
            .find(|t| t.name.eq_ignore_ascii_case(name))
            .or_else(|| self.catalog.table(name))
    }
}

/// The set of relations — and therefore columns — a query resolves against.
pub(crate) struct Scope<'a> {
    env: Env<'a>,
    sources: Vec<Source>,
}

impl<'a> Scope<'a> {
    /// Builds the scope from a `FROM` clause. A derived table's own placeholders
    /// are appended to `params` in textual order.
    pub fn build(
        query: &str,
        from: &[TableWithJoins],
        env: &Env<'a>,
        params: &mut Vec<Param>,
    ) -> Result<Scope<'a>, Error> {
        let mut sources = Vec::new();
        for twj in from {
            add_relation(query, &twj.relation, false, &mut sources, env, params)?;
            for join in &twj.joins {
                add_join(query, join, &mut sources, env, params)?;
            }
        }
        if sources.is_empty() {
            return Err(Error::Unsupported {
                name: query.to_string(),
                reason: "query has no table to read from".to_string(),
            });
        }
        Ok(Scope {
            env: env.clone(),
            sources,
        })
    }

    /// A scope of exactly one table — used to resolve a `RETURNING` list
    /// against the target of an `INSERT`/`UPDATE`/`DELETE`.
    pub fn single(catalog: &'a Catalog, table: &Table) -> Scope<'a> {
        Scope {
            env: Env::root(catalog),
            sources: vec![Source {
                alias: table.name.clone(),
                table: table.clone(),
                nullable_by_join: false,
            }],
        }
    }

    /// The environment of a subquery nested in this scope: same CTEs, with this
    /// scope's relations as the nearest enclosing scope.
    pub fn child_env(&self) -> Env<'a> {
        let mut env = self.env.clone();
        env.outer.insert(0, self.sources.clone());
        env.anonymous_columns = true;
        env
    }

    /// Whether a projection item may go unaliased (see [`Env`]).
    pub fn anonymous_columns(&self) -> bool {
        self.env.anonymous_columns
    }

    /// Resolves an unqualified column name — in this scope, else the nearest
    /// enclosing scope that has it — erroring if it is absent everywhere or
    /// present in more than one relation of the same scope (ambiguous).
    pub fn unqualified(&self, query: &str, col: &str) -> Result<Resolved, Error> {
        for layer in std::iter::once(&self.sources).chain(&self.env.outer) {
            if let Some(hit) = find_unqualified(query, layer, col)? {
                return Ok(hit);
            }
        }
        Err(Error::Resolve(format!(
            "query `{query}`: column `{col}` is not in any table in scope"
        )))
    }

    /// Resolves a `table.column` reference against the matching relation —
    /// this scope's first, then the enclosing scopes'.
    pub fn qualified(&self, query: &str, alias: &str, col: &str) -> Result<Resolved, Error> {
        let source = self.source(query, alias)?;
        let column = source.table.column(col).ok_or_else(|| {
            Error::Resolve(format!(
                "query `{query}`: column `{col}` not found on `{alias}`"
            ))
        })?;
        Ok(resolved(source, column))
    }

    /// Every column of every relation in scope, in order — for `SELECT *`. Each
    /// is `(source column name, resolved type)`; callers de-duplicate field names.
    pub fn all_columns(&self) -> Vec<(String, Resolved)> {
        self.sources
            .iter()
            .flat_map(|source| {
                source
                    .table
                    .columns
                    .iter()
                    .map(move |column| (column.name.clone(), resolved(source, column)))
            })
            .collect()
    }

    /// Every column of one relation/alias — for a qualified `alias.*`.
    pub fn table_columns(
        &self,
        query: &str,
        alias: &str,
    ) -> Result<Vec<(String, Resolved)>, Error> {
        let source = self.source(query, alias)?;
        Ok(source
            .table
            .columns
            .iter()
            .map(|column| (column.name.clone(), resolved(source, column)))
            .collect())
    }

    fn source(&self, query: &str, alias: &str) -> Result<&Source, Error> {
        std::iter::once(&self.sources)
            .chain(&self.env.outer)
            .flatten()
            .find(|s| s.alias.eq_ignore_ascii_case(alias))
            .ok_or_else(|| {
                Error::Resolve(format!(
                    "query `{query}`: no table or alias `{alias}` in scope"
                ))
            })
    }
}

/// Looks `col` up in one scope level: `Ok(None)` when no relation has it, an
/// error when more than one does.
fn find_unqualified(query: &str, sources: &[Source], col: &str) -> Result<Option<Resolved>, Error> {
    let mut hit: Option<Resolved> = None;
    for source in sources {
        if let Some(column) = source.table.column(col) {
            if hit.is_some() {
                return Err(Error::Resolve(format!(
                    "query `{query}`: column `{col}` is ambiguous across the tables in scope; \
                     qualify it (`table.{col}`)"
                )));
            }
            hit = Some(resolved(source, column));
        }
    }
    Ok(hit)
}

/// Column nullability folds schema `NOT NULL` together with any outer-join
/// nullability on its relation.
fn resolved(source: &Source, column: &Column) -> Resolved {
    Resolved {
        ty: column.ty,
        nullable: !column.not_null || source.nullable_by_join,
    }
}

/// Adds a `FROM`/`JOIN` relation to the scope. `nullable` is whether the join
/// that introduced it makes its columns optional.
fn add_relation(
    query: &str,
    relation: &TableFactor,
    nullable: bool,
    sources: &mut Vec<Source>,
    env: &Env,
    params: &mut Vec<Param>,
) -> Result<(), Error> {
    match relation {
        TableFactor::Table { name, alias, .. } => {
            let table_name = last_ident(name);
            let table = env.relation(&table_name).ok_or_else(|| {
                Error::Resolve(format!("query `{query}`: unknown table `{table_name}`"))
            })?;
            let mut table = table.clone();
            if let Some(alias) = alias {
                rename_columns(query, &mut table, &alias.columns)?;
            }
            let alias = alias
                .as_ref()
                .map_or_else(|| table.name.clone(), |a| a.name.value.clone());
            sources.push(Source {
                alias,
                table,
                nullable_by_join: nullable,
            });
            Ok(())
        }
        TableFactor::Derived {
            lateral: false,
            subquery,
            alias: Some(alias),
        } => {
            let columns = query::query_columns(query, subquery, &env.named_columns(), params)?;
            let mut table = query::result_table(&alias.name.value, &columns);
            rename_columns(query, &mut table, &alias.columns)?;
            sources.push(Source {
                alias: alias.name.value.clone(),
                table,
                nullable_by_join: nullable,
            });
            Ok(())
        }
        TableFactor::Derived { lateral: true, .. } => Err(Error::Unsupported {
            name: query.to_string(),
            reason: "a LATERAL derived table is not supported".to_string(),
        }),
        TableFactor::Derived { alias: None, .. } => Err(Error::Unsupported {
            name: query.to_string(),
            reason: "a derived table needs an alias (`(SELECT …) AS name`)".to_string(),
        }),
        _ => Err(Error::Unsupported {
            name: query.to_string(),
            reason: "FROM/JOIN must name a table, a CTE, or a derived table (subquery)".to_string(),
        }),
    }
}

/// Applies an `AS name(a, b, …)` column-alias list to a relation's columns.
pub(crate) fn rename_columns(query: &str, table: &mut Table, names: &[Ident]) -> Result<(), Error> {
    if names.is_empty() {
        return Ok(());
    }
    if names.len() != table.columns.len() {
        return Err(Error::Resolve(format!(
            "query `{query}`: `{}` has {} column(s) but its alias lists {}",
            table.name,
            table.columns.len(),
            names.len()
        )));
    }
    for (column, name) in table.columns.iter_mut().zip(names) {
        column.name.clone_from(&name.value);
    }
    Ok(())
}

/// Adds a joined relation, applying the outer-join nullability rule: a `LEFT`
/// join nulls the joined relation, a `RIGHT` join nulls the relations already in
/// scope, a `FULL` join nulls both. Inner and cross joins null neither.
fn add_join(
    query: &str,
    join: &Join,
    sources: &mut Vec<Source>,
    env: &Env,
    params: &mut Vec<Param>,
) -> Result<(), Error> {
    let (joined_nullable, existing_nullable) = match join.join_operator {
        JoinOperator::LeftOuter(_) => (true, false),
        JoinOperator::RightOuter(_) => (false, true),
        JoinOperator::FullOuter(_) => (true, true),
        _ => (false, false),
    };
    if existing_nullable {
        for source in sources.iter_mut() {
            source.nullable_by_join = true;
        }
    }
    add_relation(query, &join.relation, joined_nullable, sources, env, params)
}
