//! neoc autosql — generate a type-safe neoc data-access layer from a SQL
//! schema and a set of single-query files (sqlc-style, convention-driven).
//!
//! The generator is a compiler-domain tool: it parses SQL with `sqlparser` and
//! emits **`.neoc` source** that couples to no runtime module. The only seam to
//! a database is a user-written `impl SqlExecutor` (declared via `ext`).
//! autosql's job ends at producing valid neoc; lowering that neoc to a target
//! is the codegen backends' concern, not autosql's.
//!
//! # Output
//!
//! [`generate_project`] returns a **set of `.neoc` files** rooted at an
//! `autosql/` directory:
//!
//! * `ports.neoc` — the shared contract: the `SqlValue` enum, the
//!   `SqlExecutor` trait, and the `decode_*` row helpers. The one file every
//!   other generated file imports.
//! * `migrate.neoc` — a `migrate` function that runs every DDL statement, in
//!   migration order, through `SqlExecutor::execute`.
//! * one file per query — its own row `struct`, its row decoder, and the typed
//!   query function — each importing only `autosql::ports`.
//!
//! The user implements `SqlExecutor` for their runtime via `ext`.
//!
//! # Example
//!
//! ```
//! use neoc_autosql::{generate_project, Dialect, Migration, Query};
//!
//! let migrations = [Migration {
//!     order: 1,
//!     name: "init".into(),
//!     sql: "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);".into(),
//! }];
//! let queries = [Query {
//!     name: "user_by_id".into(),
//!     sql: "SELECT id, name FROM users WHERE id = ?;".into(),
//! }];
//! let files = generate_project(&migrations, &queries, Dialect::Sqlite).unwrap();
//! assert!(files.iter().any(|f| f.relative_path.ends_with("ports.neoc")));
//! assert!(files.iter().any(|f| f.relative_path.ends_with("migrate.neoc")));
//! assert!(files.iter().any(|f| f.relative_path.ends_with("user_by_id.neoc")));
//! ```

mod catalog;
mod emit;
mod expr;
mod naming;
mod query;
mod scope;

use std::path::PathBuf;

use catalog::Catalog;
use query::AnalyzedQuery;

/// One DDL migration file: its numeric order, its descriptive name, and the
/// raw SQL it contains. A file may hold several statements separated by `;`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migration {
    /// The leading `{n}` of the file name, deciding apply order (ascending).
    pub order: u32,
    /// The descriptive remainder of the file name (e.g. `init`), for comments.
    pub name: String,
    /// The raw DDL — one or more `;`-separated statements.
    pub sql: String,
}

/// One DML query file: the query name (already the neoc identifier base) and
/// the single SQL statement it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// The query name — the file stem in `snake_case`, used for the generated
    /// file name, the function name, and the row struct.
    pub name: String,
    /// The single SQL statement. Its kind decides the return shape: a `SELECT`
    /// returns `[Row]`, anything else returns the rows-affected `number`.
    pub sql: String,
}

/// One generated `.neoc` file: where it goes (relative to the `autosql/`
/// parent — i.e. under the project source root) and its contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    /// The path relative to the project source root, e.g.
    /// `autosql/ports.neoc` or `autosql/user_by_id.neoc`.
    pub relative_path: PathBuf,
    /// The full `.neoc` source for the file.
    pub contents: String,
}

/// The SQL dialect to parse against. Selects the `sqlparser` dialect and (in a
/// future revision) the placeholder rendering and type catalog. For sqlc-style
/// generation the query text is emitted verbatim, so today the dialect only
/// affects how the schema and queries are *parsed*.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Dialect {
    /// `SQLite` (`?` placeholders). The default.
    #[default]
    Sqlite,
    /// `PostgreSQL` (`$1` placeholders).
    Postgres,
    /// `MySQL` (`?` placeholders).
    Mysql,
    /// Dialect-agnostic superset.
    Generic,
}

impl Dialect {
    /// Parses `sql` into statements using this dialect's grammar.
    fn parse(self, sql: &str) -> Result<Vec<sqlparser::ast::Statement>, Error> {
        use sqlparser::dialect::{GenericDialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect};
        use sqlparser::parser::Parser;
        let result = match self {
            Dialect::Sqlite => Parser::parse_sql(&SQLiteDialect {}, sql),
            Dialect::Postgres => Parser::parse_sql(&PostgreSqlDialect {}, sql),
            Dialect::Mysql => Parser::parse_sql(&MySqlDialect {}, sql),
            Dialect::Generic => Parser::parse_sql(&GenericDialect {}, sql),
        };
        result.map_err(|e| Error::Sql(e.to_string()))
    }
}

impl std::str::FromStr for Dialect {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "sqlite" => Ok(Dialect::Sqlite),
            "postgres" | "postgresql" => Ok(Dialect::Postgres),
            "mysql" => Ok(Dialect::Mysql),
            "generic" => Ok(Dialect::Generic),
            other => Err(format!(
                "unknown dialect `{other}` (expected `sqlite`, `postgres`, `mysql`, or `generic`)"
            )),
        }
    }
}

impl std::fmt::Display for Dialect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Dialect::Sqlite => "sqlite",
            Dialect::Postgres => "postgres",
            Dialect::Mysql => "mysql",
            Dialect::Generic => "generic",
        })
    }
}

/// A failure during generation. Messages speak SQL and neoc — never the host
/// language or `sqlparser` internals beyond the raw SQL error text.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The schema or a query failed to parse as SQL.
    #[error("SQL parse error: {0}")]
    Sql(String),
    /// A query used a construct autosql does not yet generate code for.
    #[error("unsupported query `{name}`: {reason}")]
    Unsupported {
        /// The annotated query name.
        name: String,
        /// What was unsupported, in plain terms.
        reason: String,
    },
    /// A query or schema referenced a table or column that the schema does not
    /// declare.
    #[error("{0}")]
    Resolve(String),
    /// A query's inferred parameters do not match the bind placeholders in its
    /// SQL: a placeholder sits in a clause autosql cannot bind from, or an
    /// `INSERT`'s column list and its `VALUES` placeholders disagree. Binding it
    /// would pass the wrong number of values at runtime, so generation fails.
    #[error(
        "query `{name}`: SQL has {placeholders} bind placeholder(s) but \
         {params} parameter(s) were inferred — every placeholder must be \
         bindable, and an INSERT's columns must match its VALUES"
    )]
    PlaceholderMismatch {
        /// The annotated query name.
        name: String,
        /// Bind placeholders counted across the parsed statement.
        placeholders: usize,
        /// Parameters autosql inferred from the clauses it binds.
        params: usize,
    },
}

/// Generates the `autosql/` neoc data-access layer from a set of `migrations`
/// and `queries`.
///
/// The schema catalog query inference resolves against is built from every
/// migration's DDL, in ascending `order`. Each query is then parsed and typed
/// against that catalog, and the result is one `ports.neoc`, one
/// `migrate.neoc`, and one file per query.
///
/// # Arguments
///
/// * `migrations` — the DDL files. They are sorted by `order` (ascending)
///   before the catalog is built and the migration runner is emitted; the
///   slice need not arrive sorted.
/// * `queries` — one SQL statement per entry, named by `name`.
/// * `dialect` — the SQL grammar to parse against.
///
/// # Returns
///
/// A [`GeneratedFile`] per output, each carrying a path relative to the project
/// source root (under `autosql/`) and its `.neoc` source.
///
/// # Errors
///
/// Returns [`Error`] if any SQL fails to parse, a query names an unknown table
/// or column, or a query uses an unsupported construct (see `ASSUMPTIONS.md`
/// for the v1 scope).
pub fn generate_project(
    migrations: &[Migration],
    queries: &[Query],
    dialect: Dialect,
) -> Result<Vec<GeneratedFile>, Error> {
    let mut migrations: Vec<&Migration> = migrations.iter().collect();
    migrations.sort_by_key(|m| m.order);

    // The catalog ignores non-`CREATE TABLE` statements, so the concatenated
    // DDL of every migration (in order) is a valid schema to resolve against.
    let schema = migrations
        .iter()
        .map(|m| m.sql.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let catalog = Catalog::from_schema(&schema, dialect)?;

    let analyzed = queries
        .iter()
        .map(|q| AnalyzedQuery::analyze(&q.name, &q.sql, &catalog, dialect))
        .collect::<Result<Vec<_>, _>>()?;

    let dir = PathBuf::from("autosql");
    let mut files = vec![
        GeneratedFile {
            relative_path: dir.join("ports.neoc"),
            contents: emit::ports(),
        },
        GeneratedFile {
            relative_path: dir.join("migrate.neoc"),
            contents: emit::migrate(&migrations),
        },
    ];
    for query in &analyzed {
        files.push(GeneratedFile {
            relative_path: dir.join(format!("{}.neoc", query.fn_name())),
            contents: emit::query_file(query),
        });
    }
    Ok(files)
}

/// The base neoc type a column maps to. Nullability is tracked separately and
/// wraps this in `Option<T>` at emit time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Prim {
    Number,
    String,
    Bool,
}

impl Prim {
    /// The neoc spelling of this primitive.
    fn neoc(self) -> &'static str {
        match self {
            Prim::Number => "number",
            Prim::String => "string",
            Prim::Bool => "bool",
        }
    }
}

/// The `SqlValue` variant a column binds through. Decoding a `number` accepts
/// both `Int` and `Real`, so the variant only drives *binding* and the decoder
/// error text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Variant {
    Int,
    Real,
    Text,
    Bool,
}

impl Variant {
    /// The `SqlValue::…` constructor name.
    fn ctor(self) -> &'static str {
        match self {
            Variant::Int => "Int",
            Variant::Real => "Real",
            Variant::Text => "Text",
            Variant::Bool => "Bool",
        }
    }
}

/// A column's neoc-facing type: its primitive and the `SqlValue` arm it uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ColType {
    prim: Prim,
    variant: Variant,
}

impl ColType {
    /// A column type for a known primitive, with the natural `SqlValue` arm
    /// (`number`→`Int`, `string`→`Text`, `bool`→`Bool`). Used when a type comes
    /// from an expression or function rather than a schema column.
    pub(crate) fn of(prim: Prim) -> ColType {
        let variant = match prim {
            Prim::Number => Variant::Int,
            Prim::String => Variant::Text,
            Prim::Bool => Variant::Bool,
        };
        ColType { prim, variant }
    }

    /// Maps a SQL data type to a neoc column type by type-name keyword.
    ///
    /// Keyword matching (rather than enumerating every `sqlparser` `DataType`
    /// variant) keeps the mapping resilient across dialects and parser
    /// versions. Unrecognised types fall back to `string`.
    fn from_sql(data_type: &sqlparser::ast::DataType) -> ColType {
        let s = data_type.to_string().to_lowercase();
        let has = |kw: &str| s.contains(kw);
        if has("bool") {
            ColType {
                prim: Prim::Bool,
                variant: Variant::Bool,
            }
        } else if has("real") || has("double") || has("float") || has("decimal") || has("numeric") {
            ColType {
                prim: Prim::Number,
                variant: Variant::Real,
            }
        } else if has("int") || s == "serial" || s == "bigserial" {
            ColType {
                prim: Prim::Number,
                variant: Variant::Int,
            }
        } else {
            // char/text/varchar/clob/uuid/json/date/time/blob/binary/… → string.
            ColType {
                prim: Prim::String,
                variant: Variant::Text,
            }
        }
    }
}
