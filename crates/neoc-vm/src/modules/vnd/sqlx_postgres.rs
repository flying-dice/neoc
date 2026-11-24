//! PostgreSQL: `vnd:sqlx_postgres` (the `sqlx` crate's pure-Rust driver).
//!
//! A connection string is a standard Postgres URL: `postgres://user:pass@host:5432/dbname`. TLS is not in this
//! build: connect to plaintext servers only (`vnd:hyper` is the only module with a TLS stack).
//!
//! Postgres numbers its placeholders: `$1, $2, ...` (not `?`).
//!
//! ```lua
//! local sqlx_postgres = require("vnd:sqlx_postgres")
//! local db = assert(sqlx_postgres.open("postgres://postgres@127.0.0.1/test"))
//! db:execute("CREATE TABLE users (id INT PRIMARY KEY, name TEXT)")
//! db:execute("INSERT INTO users (id, name) VALUES ($1, $2)", 1, "alice")
//! local rows = assert(db:query("SELECT id, name FROM users"))
//! db:close()
//! ```
//!
//! The methods are async: a call yields the calling coroutine until the query is done. Connections are pooled
//! (sqlx's defaults: at most 10, a 30 second wait for a free one), so one `Database` is safe to share between
//! coroutines that query at once.
//!
//! Parameters: the arguments after the SQL are the values for the statement's placeholders, in order: the first
//! binds `$1`. Fewer values than placeholders is the server's `database` error; a value past the last placeholder
//! is not used (Postgres takes a parameter a statement does not use). A boolean binds as `BOOL`, a whole number as `INT8`, any other number as `FLOAT8`, a string as
//! `TEXT` when it is UTF-8 and as `BYTEA` otherwise. A `nil` binds `NULL`, the last one too, as an `INT8`:
//! Postgres types its parameters, and refuses that `NULL` for a `TEXT` column, so for anything but a number
//! prefer `NULL` written in the SQL. Casts work as usual: `INSERT ... VALUES ($1::int)`.
//!
//! Rows: a row is a table from column name to value. `BOOL` is a boolean, `INT2` / `INT4` / `INT8` and `FLOAT4` /
//! `FLOAT8` are numbers, `BYTEA` is a string byte for byte, and `TEXT` / `VARCHAR` / `CHAR(n)` / `NAME` / `citext`
//! are strings. `NULL` is `nil` (the key is absent). A column of any other type is the error
//! `unsupported_column`: cast it to text in the SQL (`SELECT created_at::text`, `SELECT data::text` for `JSON` /
//! `JSONB`, `SELECT id::text` for `UUID`).
//!
//! The API's rules:
//! - A parameter that is not a boolean, a number or a string (and SQL that is not a string) is refused as an argument.
//! - A failure is a typed error: `OpenError` (`invalid_url`, `connect`) and `Error` (`closed`, `constraint`,
//!   `database`, `connection`, `pool_timed_out`, `unsupported_column`, `decode`, `other`).
//! - A column of an unsupported type, and a value that does not decode, are returned errors (`unsupported_column` with
//!   the column and its type, `decode`).

/// Column types that decode as a Lua string: the TEXT family (`CHAR(n)` reports as `CHAR`), `UNKNOWN` and
/// `citext`. These are the types sqlx lets `String` decode. Every other type without a number, boolean or bytes
/// form (NUMERIC, JSON, JSONB, UUID, dates and times, arrays) is `unsupported_column`; callers cast to text in SQL.
fn is_text_type(type_name: &str) -> bool {
    matches!(type_name, "TEXT" | "VARCHAR" | "CHAR" | "NAME" | "UNKNOWN" | "citext")
}

#[mlua_ir::module("vnd:sqlx_postgres")]
pub mod sqlx_postgres {
    use std::collections::HashMap;
    use std::str::FromStr;

    use mlua_ir::{Bytes, LuaError, Object, OneOf, Shape, Variadic};
    use serde::{Deserialize, Serialize};
    use sqlx::error::ErrorKind;
    use sqlx::pool::PoolOptions;
    use sqlx::postgres::{PgArguments, PgConnectOptions, PgPool, PgRow, Postgres};
    use sqlx::query::Query;
    use sqlx::{AssertSqlSafe, Column, Row as _, TypeInfo, ValueRef};

    use super::is_text_type;

    /// 2^63: the first whole number past what an INT8 holds.
    const INT8_END: f64 = 9_223_372_036_854_775_808.0;

    /// A value bound to a `$n` placeholder. A whole number binds as `INT8`, any other number as `FLOAT8`, a
    /// string as `TEXT` when it is UTF-8 and as `BYTEA` otherwise.
    #[derive(Debug, Clone, OneOf)]
    pub enum Param {
        /// `BOOL`.
        Boolean(bool),
        /// `INT8` when whole, `FLOAT8` otherwise.
        Number(f64),
        /// `TEXT` when UTF-8, `BYTEA` otherwise.
        Text(Bytes),
    }

    /// A value of a row.
    #[derive(Debug, Clone, OneOf)]
    pub enum Cell {
        /// A `BOOL` column.
        Boolean(bool),
        /// An `INT2`, `INT4`, `INT8`, `FLOAT4` or `FLOAT8` column.
        Number(f64),
        /// A `BYTEA` column byte for byte, or a column of the TEXT family.
        Text(Bytes),
    }

    /// A row: column name to value. A `NULL` column is absent (`nil`).
    pub type Row = HashMap<String, Cell>;

    /// Why a database could not be opened.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum OpenError {
        /// The URL is not a Postgres URL.
        #[error("{reason}")]
        InvalidUrl {
            /// What is wrong with it.
            reason: String,
        },
        /// No connection was made: nothing answers at the address, or the server refused the user or the database.
        #[error("{reason}")]
        Connect {
            /// What the driver or the server said.
            reason: String,
        },
    }

    /// Which constraint refused a change.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Violation {
        /// A UNIQUE or PRIMARY KEY constraint.
        Unique,
        /// A FOREIGN KEY constraint.
        ForeignKey,
        /// A NOT NULL constraint.
        NotNull,
        /// A CHECK constraint.
        Check,
        /// An EXCLUDE constraint.
        Exclusion,
    }

    /// Why a statement failed.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum Error {
        /// `close` was called on this database.
        #[error("database is closed")]
        Closed,
        /// A constraint refused the change.
        #[error("{reason}")]
        Constraint {
            /// Which constraint it was.
            violation: Violation,
            /// What the server said.
            reason: String,
            /// The SQLSTATE the server gave (`23505` for a unique violation).
            code: Option<String>,
        },
        /// The server refused the statement: a syntax error, a missing table, a parameter of the wrong type, fewer
        /// values than placeholders.
        #[error("{reason}")]
        Database {
            /// What the server said.
            reason: String,
            /// The SQLSTATE the server gave (`42601` for a syntax error).
            code: Option<String>,
        },
        /// The connection to the server failed or broke.
        #[error("{reason}")]
        Connection {
            /// What the driver said.
            reason: String,
        },
        /// Every connection of the pool stayed busy for as long as a call waits for one.
        #[error("timed out waiting for a free connection")]
        PoolTimedOut,
        /// A column has a type with no Lua form.
        #[error("unsupported column type {type_name} of column {column}; cast to text in SQL")]
        UnsupportedColumn {
            /// The column's name.
            column: String,
            /// The column's Postgres type (`NUMERIC`, `JSONB`, `TIMESTAMPTZ`).
            type_name: String,
        },
        /// A column's value could not be read as what its type says.
        #[error("column {column}: {reason}")]
        Decode {
            /// The column's name.
            column: String,
            /// What the driver said.
            reason: String,
        },
        /// Anything else the driver reported.
        #[error("{reason}")]
        Other {
            /// What went wrong.
            reason: String,
        },
    }

    impl From<sqlx::Error> for Error {
        fn from(error: sqlx::Error) -> Self {
            match error {
                sqlx::Error::Database(refusal) => {
                    let reason = refusal.message().to_owned();
                    let code = refusal.code().map(|code| code.into_owned());
                    let violation = match refusal.kind() {
                        ErrorKind::UniqueViolation => Violation::Unique,
                        ErrorKind::ForeignKeyViolation => Violation::ForeignKey,
                        ErrorKind::NotNullViolation => Violation::NotNull,
                        ErrorKind::CheckViolation => Violation::Check,
                        ErrorKind::ExclusionViolation => Violation::Exclusion,
                        _ => return Error::Database { reason, code },
                    };
                    Error::Constraint { violation, reason, code }
                }
                sqlx::Error::PoolClosed => Error::Closed,
                sqlx::Error::PoolTimedOut => Error::PoolTimedOut,
                sqlx::Error::Io(_) | sqlx::Error::Tls(_) | sqlx::Error::Protocol(_) => {
                    Error::Connection { reason: error.to_string() }
                }
                other => Error::Other { reason: other.to_string() },
            }
        }
    }

    /// `sql` with `params` bound, a `None` as `NULL`. The SQL is the caller's by design (`AssertSqlSafe`):
    /// running SQL text is what this module is for, and values reach the server only as bound parameters.
    fn bound(sql: &str, params: Vec<Option<Param>>) -> Query<'_, Postgres, PgArguments> {
        let mut query = sqlx::query(AssertSqlSafe(sql));
        for value in params {
            query = match value {
                None => query.bind(Option::<i64>::None),
                Some(Param::Boolean(flag)) => query.bind(flag),
                Some(Param::Number(number))
                    if number.fract() == 0.0 && (-INT8_END..INT8_END).contains(&number) =>
                {
                    // whole and in range: the cast is exact
                    #[allow(clippy::cast_possible_truncation)]
                    query.bind(number as i64)
                }
                Some(Param::Number(number)) => query.bind(number),
                Some(Param::Text(Bytes(bytes))) => match String::from_utf8(bytes) {
                    Ok(text) => query.bind(text),
                    Err(not_text) => query.bind(not_text.into_bytes()),
                },
            };
        }
        query
    }

    fn read_row(row: &PgRow) -> Result<Row, Error> {
        let columns = row.columns();
        let mut out = Row::with_capacity(columns.len());
        for column in columns {
            let index = column.ordinal();
            let decode = |error: sqlx::Error| Error::Decode {
                column: column.name().to_owned(),
                reason: error.to_string(),
            };
            let raw = row.try_get_raw(index).map_err(decode)?;
            if raw.is_null() {
                continue;
            }
            // Postgres is strict about widths (INT4 through i32, FLOAT4 through f32): each type is read as itself
            let cell = match raw.type_info().name() {
                "BOOL" => Cell::Boolean(row.try_get(index).map_err(decode)?),
                "INT2" => Cell::Number(f64::from(row.try_get::<i16, _>(index).map_err(decode)?)),
                "INT4" => Cell::Number(f64::from(row.try_get::<i32, _>(index).map_err(decode)?)),
                // a Lua number is a double: an INT8 past 2^53 loses its low bits
                #[allow(clippy::cast_precision_loss)]
                "INT8" => Cell::Number(row.try_get::<i64, _>(index).map_err(decode)? as f64),
                "FLOAT4" => Cell::Number(f64::from(row.try_get::<f32, _>(index).map_err(decode)?)),
                "FLOAT8" => Cell::Number(row.try_get(index).map_err(decode)?),
                "BYTEA" => Cell::Text(Bytes(row.try_get(index).map_err(decode)?)),
                text if is_text_type(text) => {
                    Cell::Text(Bytes(row.try_get::<String, _>(index).map_err(decode)?.into_bytes()))
                }
                other => {
                    return Err(Error::UnsupportedColumn {
                        column: column.name().to_owned(),
                        type_name: other.to_owned(),
                    });
                }
            };
            out.insert(column.name().to_owned(), cell);
        }
        Ok(out)
    }

    /// A pool of connections to one Postgres database.
    #[derive(Clone, Object)]
    pub struct Database {
        pool: PgPool,
    }

    /// Open a pool of connections to a Postgres database, and make the first. `url` is a standard Postgres URL:
    /// `postgres://user:pass@host:5432/dbname`.
    ///
    /// ```luau
    /// local db, err = sqlx_postgres.open("not-a-url")
    /// assert(db == nil and err ~= nil and err.kind == "invalid_url")
    /// ```
    #[host("Wait")]
    pub async fn open(url: String) -> Result<Database, OpenError> {
        let options = PgConnectOptions::from_str(&url)
            .map_err(|error| OpenError::InvalidUrl { reason: error.to_string() })?;
        match PoolOptions::<Postgres>::new().connect_with(options).await {
            Ok(pool) => Ok(Database { pool }),
            Err(error) => Err(OpenError::Connect { reason: error.to_string() }),
        }
    }

    #[mlua_ir::object]
    impl Database {
        /// Run one statement; how many rows it changed.
        #[host("Wait")]
        pub async fn execute(&self, sql: String, params: Variadic<Option<Param>>) -> Result<u64, Error> {
            if self.pool.is_closed() {
                return Err(Error::Closed);
            }
            Ok(bound(&sql, params.0).execute(&self.pool).await?.rows_affected())
        }

        /// Run one SELECT; every row, in order. A column of a type with no Lua form is `unsupported_column`: cast
        /// it to text in the SQL.
        #[host("Wait")]
        pub async fn query(&self, sql: String, params: Variadic<Option<Param>>) -> Result<Vec<Row>, Error> {
            if self.pool.is_closed() {
                return Err(Error::Closed);
            }
            bound(&sql, params.0).fetch_all(&self.pool).await?.iter().map(read_row).collect()
        }

        /// Run one SELECT; its first row, or `nil` when it has none. A column of a type with no Lua form is
        /// `unsupported_column`: cast it to text in the SQL.
        #[host("Wait")]
        pub async fn query_one(
            &self,
            sql: String,
            params: Variadic<Option<Param>>,
        ) -> Result<Option<Row>, Error> {
            if self.pool.is_closed() {
                return Err(Error::Closed);
            }
            bound(&sql, params.0).fetch_optional(&self.pool).await?.as_ref().map(read_row).transpose()
        }

        /// Close every connection of the pool, waiting for the ones in use. Later calls on this database fail
        /// with `closed`; closing again does nothing.
        #[host("Wait")]
        pub async fn close(&self) {
            self.pool.close().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_text_type;
    use neoc_vm_core::make_engine;

    #[test]
    fn text_family_decodes_as_string() {
        for t in ["TEXT", "VARCHAR", "CHAR", "NAME", "UNKNOWN", "citext"] {
            assert!(is_text_type(t), "{t}");
        }
    }

    #[test]
    fn other_types_are_rejected() {
        for t in ["NUMERIC", "TIMESTAMPTZ", "INTERVAL", "INT4[]", "MONEY", "JSON", "JSONB", "UUID"] {
            assert!(!is_text_type(t), "{t}");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_malformed_url_is_a_typed_error() {
        let lua = make_engine().unwrap();
        let (kind, op, text): (String, String, String) = lua
            .load(
                r#"
                local db, err = require("vnd:sqlx_postgres").open("not-a-url")
                assert(db == nil)
                return err.kind, err.op, tostring(err)
            "#,
            )
            .eval_async()
            .await
            .unwrap();
        assert_eq!(kind, "invalid_url");
        assert_eq!(op, "vnd:sqlx_postgres.open");
        assert!(text.starts_with("vnd:sqlx_postgres.open: invalid_url: "), "{text}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_url_that_is_not_a_string_is_refused_as_an_argument() {
        let lua = make_engine().unwrap();
        let text: String = lua
            .load(
                r#"
                local ok, refused = pcall(require("vnd:sqlx_postgres").open, 5)
                assert(not ok)
                return tostring(refused)
            "#,
            )
            .eval_async()
            .await
            .unwrap();
        assert!(text.contains("vnd:sqlx_postgres.open: argument #1 'url'"), "{text}");
    }

    /// The module against a real server. `URL` is the server.
    const SERVER_SCRIPT: &str = r#"
        local sqlx_postgres = require("vnd:sqlx_postgres")

        -- open
        local db, open_err = sqlx_postgres.open(URL)
        assert(open_err == nil and db ~= nil)

        -- execute: rows affected
        db:execute("DROP TABLE IF EXISTS exec_t")
        local n, err = db:execute("CREATE TABLE exec_t (id INT PRIMARY KEY, v TEXT)")
        assert(err == nil and n == 0)
        n, err = db:execute("INSERT INTO exec_t (id, v) VALUES ($1, $2)", 1, "alice")
        assert(err == nil and n == 1)
        assert(db:execute("INSERT INTO exec_t VALUES (2, 'a'), (3, 'b'), (4, 'a')") == 3)
        assert(db:execute("UPDATE exec_t SET v = $1 WHERE v = $2", "x", "a") == 2)

        -- invalid SQL
        local bad, bad_err = db:execute("NOT VALID SQL")
        assert(bad == nil and bad_err.kind == "database" and bad_err.op == "vnd:sqlx_postgres.Database:execute")
        assert(bad_err.code == "42601")

        -- a constraint
        local dup, dup_err = db:execute("INSERT INTO exec_t (id, v) VALUES ($1, $2)", 1, "again")
        assert(dup == nil and dup_err.kind == "constraint" and dup_err.violation == "unique" and dup_err.code == "23505")

        -- nil binds as NULL (an INT8 on Postgres: the column must be a number), the last value too
        db:execute("DROP TABLE IF EXISTS exec_null")
        db:execute("CREATE TABLE exec_null (id INT PRIMARY KEY, v BIGINT)")
        assert(db:execute("INSERT INTO exec_null (id, v) VALUES ($1, $2)", 1, nil) == 1)
        assert(db:execute("INSERT INTO exec_null (v, id) VALUES ($1, $2)", nil, 2) == 1)
        local null_rows = assert(db:query("SELECT id, v FROM exec_null ORDER BY id"))
        assert(#null_rows == 2 and null_rows[1].id == 1 and null_rows[1].v == nil)
        assert(null_rows[2].id == 2 and null_rows[2].v == nil)
        -- fewer values than placeholders: the server refuses the statement
        local short, short_err = db:execute("INSERT INTO exec_null (id, v) VALUES ($1, $2)", 3)
        assert(short == nil and short_err.kind == "database")
        -- more values than placeholders: Postgres takes a parameter a statement does not use, so the statement runs
        -- and the values past its last placeholder are not used
        assert(db:execute("INSERT INTO exec_null (id) VALUES ($1)", 3, nil) == 1)
        assert(db:execute("DELETE FROM exec_null WHERE id = $1", 3, "unused") == 1)

        -- query
        local rows, query_err = db:query("SELECT id, v FROM exec_t ORDER BY id")
        assert(query_err == nil and #rows == 4)
        assert(rows[1].id == 1 and rows[1].v == "alice")
        assert(rows[2].id == 2 and rows[2].v == "x")
        assert(#assert(db:query("SELECT * FROM exec_t WHERE id > 99")) == 0)
        local where = assert(db:query("SELECT v FROM exec_t WHERE v = $1", "b"))
        assert(#where == 1 and where[1].v == "b")

        -- the types of values
        db:execute("DROP TABLE IF EXISTS q_types")
        db:execute("CREATE TABLE q_types (id INT PRIMARY KEY, x DOUBLE PRECISION, b BYTEA, f BOOL, c CHAR(3), big BIGINT, s SMALLINT, r REAL)")
        assert(db:execute("INSERT INTO q_types VALUES (1, $1, $2, $3, 'abc', $4, 7, 0.5)", 3.14, "\0\1\2\254\255", true, 2 ^ 40) == 1)
        local typed = assert(db:query_one("SELECT x, b, f, c, big, s, r FROM q_types WHERE id = 1"))
        assert(typed.x == 3.14 and typed.b == "\0\1\2\254\255" and typed.f == true and typed.c == "abc")
        assert(typed.big == 2 ^ 40 and typed.s == 7 and typed.r == 0.5)

        -- a column type with no Lua form is an error with a cast hint; cast to text works
        for _, q in ipairs({ "SELECT 1::numeric AS n", "SELECT '{}'::jsonb AS j" }) do
            local row, unsupported = db:query_one(q)
            assert(row == nil and unsupported.kind == "unsupported_column")
            assert(unsupported.column == "n" or unsupported.column == "j")
            assert(unsupported.type_name == "NUMERIC" or unsupported.type_name == "JSONB")
            assert(string.find(unsupported.message, "cast to text in SQL", 1, true))
            local all, all_unsupported = db:query(q)
            assert(all == nil and all_unsupported.kind == "unsupported_column")
        end
        assert(assert(db:query_one("SELECT 1::numeric::text AS n")).n == "1")

        -- query_one
        local row, one_err = db:query_one("SELECT id, v FROM exec_t WHERE id = $1", 1)
        assert(one_err == nil and row.id == 1 and row.v == "alice")
        local none, none_err = db:query_one("SELECT * FROM exec_t WHERE id = $1", 999)
        assert(none == nil and none_err == nil)

        -- close: idempotent, and everything after it fails
        db:close()
        db:close()
        local closed, closed_err = db:execute("SELECT 1")
        assert(closed == nil and closed_err.kind == "closed")
        assert(tostring(closed_err) == "vnd:sqlx_postgres.Database:execute: closed: database is closed")
    "#;

    /// Against a real server: `NEOC_TEST_POSTGRES_URL=postgres://postgres:pw@127.0.0.1:5432/postgres cargo test -p
    /// neoc-vm sqlx_postgres -- --ignored`. The script creates and drops its own tables (`exec_t`, `exec_null`,
    /// `q_types`).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "needs a Postgres server: set NEOC_TEST_POSTGRES_URL"]
    async fn against_a_server() {
        let url = std::env::var("NEOC_TEST_POSTGRES_URL")
            .expect("NEOC_TEST_POSTGRES_URL is the server to test against");
        let lua = make_engine().unwrap();
        lua.globals().set("URL", url).unwrap();
        lua.load(SERVER_SCRIPT).exec_async().await.unwrap();
    }

    /// The same script against a Postgres that `vnd:testcontainers_modules` starts for the test and stops after it,
    /// as neoc's Luau suite ran it: `cargo test -p neoc-vm sqlx_postgres -- --ignored`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "needs a running Docker daemon"]
    async fn against_a_container() {
        let lua = make_engine().unwrap();
        let start = r#"
            container = assert(require("vnd:testcontainers_modules").postgres())
            URL = container:url()
        "#;
        lua.load(start).exec_async().await.unwrap();
        let ran = lua.load(SERVER_SCRIPT).exec_async().await;
        lua.load("assert(container:stop())").exec_async().await.unwrap();
        ran.unwrap();
    }
}
