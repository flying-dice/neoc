//! Query resolution scope: the tables a single `SELECT`/`UPDATE`/`DELETE` can
//! see, with alias-aware column lookup and outer-join nullability.
//!
//! A `FROM` clause is one or more comma-joined tables, each possibly carrying
//! `JOIN`s. Each table enters the scope under its alias (or its own name). An
//! outer join can make a side's columns NULL even when the schema marks them
//! `NOT NULL`: `LEFT JOIN` nulls the joined table, `RIGHT JOIN` nulls the
//! tables already in scope, `FULL JOIN` nulls both.

use sqlparser::ast::{Join, JoinOperator, TableFactor, TableWithJoins};

use crate::catalog::{Catalog, Column, Table, last_ident};
use crate::{ColType, Error};

/// One table in scope, under its alias, with whether an outer join can NULL it.
struct Source<'a> {
    /// The alias the query refers to it by — an `AS` alias or the table name.
    alias: String,
    table: &'a Table,
    /// `true` when an outer join can make this table's columns NULL.
    nullable_by_join: bool,
}

/// A resolved column reference: its neoc type and whether it can be NULL
/// (schema nullability OR an outer join).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Resolved {
    pub ty: ColType,
    pub nullable: bool,
}

/// The set of tables — and therefore columns — a query resolves against.
pub(crate) struct Scope<'a> {
    sources: Vec<Source<'a>>,
}

impl<'a> Scope<'a> {
    /// Builds the scope from a `FROM` clause against the schema `catalog`.
    pub fn build(
        query: &str,
        from: &[TableWithJoins],
        catalog: &'a Catalog,
    ) -> Result<Scope<'a>, Error> {
        let mut sources = Vec::new();
        for twj in from {
            add_relation(query, &twj.relation, false, &mut sources, catalog)?;
            for join in &twj.joins {
                add_join(query, join, &mut sources, catalog)?;
            }
        }
        if sources.is_empty() {
            return Err(Error::Unsupported {
                name: query.to_string(),
                reason: "query has no table to read from".to_string(),
            });
        }
        Ok(Scope { sources })
    }

    /// A scope of exactly one table — used to resolve a `RETURNING` list
    /// against the target of an `INSERT`/`UPDATE`/`DELETE`.
    pub fn single(table: &'a Table) -> Scope<'a> {
        Scope {
            sources: vec![Source {
                alias: table.name.clone(),
                table,
                nullable_by_join: false,
            }],
        }
    }

    /// Resolves an unqualified column name, erroring if it is absent from every
    /// table in scope or present in more than one (ambiguous).
    pub fn unqualified(&self, query: &str, col: &str) -> Result<Resolved, Error> {
        let mut hit: Option<Resolved> = None;
        for source in &self.sources {
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
        hit.ok_or_else(|| {
            Error::Resolve(format!(
                "query `{query}`: column `{col}` is not in any table in scope"
            ))
        })
    }

    /// Resolves a `table.column` reference against the matching source.
    pub fn qualified(&self, query: &str, alias: &str, col: &str) -> Result<Resolved, Error> {
        let source = self.source(query, alias)?;
        let column = source.table.column(col).ok_or_else(|| {
            Error::Resolve(format!(
                "query `{query}`: column `{col}` not found on `{alias}`"
            ))
        })?;
        Ok(resolved(source, column))
    }

    /// Every column of every table in scope, in order — for `SELECT *`. Each is
    /// `(source column name, resolved type)`; callers de-duplicate field names.
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

    /// Every column of one table/alias — for a qualified `alias.*`.
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

    /// The lone table's name when the scope has exactly one source — lets a
    /// `SELECT` whose projection matches that table reuse its model struct.
    /// `None` for a multi-table scope.
    // TODO: clean-code - 0.55 - DEAD: sole_table is computed and threaded through Inferred but query.rs:106 binds it to `_table` and never reads it; the model-struct reuse it promises is unimplemented — wire it up or drop it.
    pub fn sole_table(&self) -> Option<String> {
        match self.sources.as_slice() {
            [source] => Some(source.table.name.clone()),
            _ => None,
        }
    }

    fn source(&self, query: &str, alias: &str) -> Result<&Source<'a>, Error> {
        self.sources
            .iter()
            .find(|s| s.alias.eq_ignore_ascii_case(alias))
            .ok_or_else(|| {
                Error::Resolve(format!(
                    "query `{query}`: no table or alias `{alias}` in scope"
                ))
            })
    }
}

/// Column nullability folds schema `NOT NULL` together with any outer-join
/// nullability on its table.
fn resolved(source: &Source, column: &Column) -> Resolved {
    Resolved {
        ty: column.ty,
        nullable: !column.not_null || source.nullable_by_join,
    }
}

/// Adds a `FROM`/`JOIN` relation to the scope. `nullable` is whether the join
/// that introduced it makes its columns optional.
fn add_relation<'a>(
    query: &str,
    relation: &TableFactor,
    nullable: bool,
    sources: &mut Vec<Source<'a>>,
    catalog: &'a Catalog,
) -> Result<(), Error> {
    match relation {
        TableFactor::Table { name, alias, .. } => {
            let table_name = last_ident(name);
            let table = catalog.table(&table_name).ok_or_else(|| {
                Error::Resolve(format!("query `{query}`: unknown table `{table_name}`"))
            })?;
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
        _ => Err(Error::Unsupported {
            name: query.to_string(),
            reason: "FROM/JOIN must name a plain table (no subqueries or derived tables)"
                .to_string(),
        }),
    }
}

/// Adds a joined table, applying the outer-join nullability rule: a `LEFT` join
/// nulls the joined table, a `RIGHT` join nulls the tables already in scope, a
/// `FULL` join nulls both. Inner and cross joins null neither.
fn add_join<'a>(
    query: &str,
    join: &Join,
    sources: &mut Vec<Source<'a>>,
    catalog: &'a Catalog,
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
    add_relation(query, &join.relation, joined_nullable, sources, catalog)
}
