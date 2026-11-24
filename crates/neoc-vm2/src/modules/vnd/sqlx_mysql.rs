//! MySQL: `vnd:sqlx_mysql` (the `sqlx` crate's pure-Rust driver).
//!
//! A connection string is a standard MySQL URL: `mysql://user:pass@host:3306/dbname`. TLS is not in this build:
//! connect to plaintext servers only (`vnd:hyper` is the only module with a TLS stack). MySQL 8.0 and later
//! usually require TLS by default; configure the server to allow plaintext, or run through a sidecar.
//!
//! ```lua
//! local sqlx_mysql = require("vnd:sqlx_mysql")
//! local db = assert(sqlx_mysql.open("mysql://root@127.0.0.1/test"))
//! db:execute("CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(64))")
//! db:execute("INSERT INTO users (id, name) VALUES (?, ?)", { 1, "alice" })
//! local rows = assert(db:query("SELECT id, name FROM users"))
//!
//! -- Hot paths: prepare once, reuse:
//! local insert = db:prepare("INSERT INTO users (id, name) VALUES (?, ?)")
//! insert:execute({ 2, "bob" })
//! db:close()
//! ```
//!
//! The methods are async: a call yields the calling coroutine until the query is done (network IO, worth yielding
//! for, unlike `vnd:rusqlite`'s sync C calls). Connections are pooled (sqlx's defaults: at most 10, a 30 second
//! wait for a free one), so one `Database` is safe to share between coroutines that query at once.
//!
//! Parameters: the second argument is the list of values for the statement's `?` placeholders, by position.
//! `params[1]` binds the first. A position with no value (`nil`) before the last one given binds `NULL`. A
//! trailing `nil` cannot be seen (a Lua list ends at its last value), so a statement whose last parameter may be
//! `NULL` either writes it in the SQL or places that parameter earlier. A boolean binds as a boolean, a whole
//! number as BIGINT, any other number as DOUBLE, a string as text when it is UTF-8 and as bytes otherwise.
//!
//! Rows: a row is a table from column name to value. `BOOLEAN` is a boolean, the integer types and `FLOAT` /
//! `DOUBLE` are numbers, the BLOB and BINARY types are strings byte for byte, and everything else (`VARCHAR`,
//! `TEXT`, `JSON`, `DECIMAL`, dates and times, `ENUM`, `SET`) is its text. `NULL` is `nil` (the key is absent).
//!
//! Ported from neoc-vm's `vnd:sqlx_mysql`. What changed on purpose:
//! - Parameters are one list (`db:execute(sql, { a, b })`, `stmt:execute({ a, b })`), not trailing arguments; a
//!   trailing `nil` in it does not bind (see above), where a trailing `nil` argument bound `NULL`.
//! - A parameter that is not a boolean, a number or a string (and SQL that is not a string) is refused as an
//!   argument; it was a returned error.
//! - A failure is a typed error: `OpenError` (`invalid_url`, `connect`) and `Error` (`closed`,
//!   `invalid_parameter`, `constraint`, `database`, `connection`, `pool_timed_out`, `decode`, `other`).
//! - A value that does not decode is the returned error `decode` with its column; it was raised.
//! - A statement's errors name `vnd:sqlx_mysql.Statement:execute` as their `op` (it was `mysql.statement.execute`).

/// What a column's value is read as, worked out once from the type's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bool,
    Integer,
    Real,
    Blob,
    Text,
}

fn classify(type_name: &str) -> Kind {
    if type_name.starts_with("BOOLEAN") {
        return Kind::Bool;
    }
    if matches!(type_name, "FLOAT" | "DOUBLE") {
        return Kind::Real;
    }
    if matches!(type_name, "BLOB" | "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" | "BINARY" | "VARBINARY") {
        return Kind::Blob;
    }
    // TINYINT/SMALLINT/MEDIUMINT/INT/BIGINT, signed or unsigned ("INT UNSIGNED"); not INTERVAL
    if type_name.contains("INT") && !type_name.contains("INTERVAL") {
        return Kind::Integer;
    }
    // VARCHAR/TEXT/CHAR/JSON/DATE/TIME/DATETIME/TIMESTAMP/DECIMAL/ENUM/SET: sqlx gives dates, times and decimals
    // as text unless their features are on, and they are not
    Kind::Text
}

#[mlua_ir::module("vnd:sqlx_mysql")]
pub mod sqlx_mysql {
    use std::collections::{BTreeMap, HashMap};
    use std::str::FromStr;
    use std::sync::Arc;

    use mlua_ir::{Bytes, LuaError, Object, OneOf, Shape};
    use serde::{Deserialize, Serialize};
    use sqlx::error::ErrorKind;
    use sqlx::mysql::{MySql, MySqlArguments, MySqlConnectOptions, MySqlPool, MySqlRow, MySqlValueRef};
    use sqlx::pool::PoolOptions;
    use sqlx::query::Query;
    use sqlx::{AssertSqlSafe, Column, ConnectOptions, Decode, Row as _, TypeInfo, ValueRef};
    use tracing::log::LevelFilter;

    use super::{Kind, classify};
    use crate::modules::vnd::by_position;

    /// sqlx keeps this many prepared statements per connection (its default is 100): enough for an application
    /// with more distinct SQL strings than that to never prepare one twice.
    const STATEMENT_CACHE_CAPACITY: usize = 1024;

    /// 2^63: the first whole number past what a BIGINT holds.
    const BIGINT_END: f64 = 9_223_372_036_854_775_808.0;

    /// A value bound to a `?` placeholder. A whole number binds as BIGINT, any other number as DOUBLE, a string as
    /// text when it is UTF-8 and as bytes otherwise.
    #[derive(Debug, Clone, OneOf)]
    pub enum Param {
        /// A boolean.
        Boolean(bool),
        /// BIGINT when whole, DOUBLE otherwise.
        Number(f64),
        /// Text when UTF-8, bytes otherwise.
        Text(Bytes),
    }

    /// The values for a statement's placeholders, by position from 1. A position with no value before the last
    /// one given binds `NULL`.
    pub type Params = BTreeMap<u32, Option<Param>>;

    /// A value of a row.
    #[derive(Debug, Clone, OneOf)]
    pub enum Cell {
        /// A `BOOLEAN` column.
        Boolean(bool),
        /// An integer, `FLOAT` or `DOUBLE` column.
        Number(f64),
        /// A BLOB or BINARY column byte for byte, or any other column as its text.
        Text(Bytes),
    }

    /// A row: column name to value. A `NULL` column is absent (`nil`).
    pub type Row = HashMap<String, Cell>;

    /// Why a database could not be opened.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum OpenError {
        /// The URL is not a MySQL URL.
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
    }

    /// Why a statement failed.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum Error {
        /// `close` was called on this database.
        #[error("database is closed")]
        Closed,
        /// A value was given at a position no statement has.
        #[error("parameter positions run from 1 to 65535, got {position}")]
        InvalidParameter {
            /// The position the value was given at.
            position: u32,
        },
        /// A constraint refused the change.
        #[error("{reason}")]
        Constraint {
            /// Which constraint it was.
            violation: Violation,
            /// What the server said.
            reason: String,
            /// The SQLSTATE the server gave (`23000`).
            code: Option<String>,
        },
        /// The server refused the statement: a syntax error, a missing table, a wrong number of parameters.
        #[error("{reason}")]
        Database {
            /// What the server said.
            reason: String,
            /// The SQLSTATE the server gave (`42000` for a syntax error).
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

    /// `sql` with `params` bound, or why they cannot be. The SQL is the caller's by design (`AssertSqlSafe`):
    /// running SQL text is what this module is for, and values reach the server only as bound parameters.
    fn bound(sql: &str, params: Option<Params>) -> Result<Query<'_, MySql, MySqlArguments>, Error> {
        let values = by_position(params).map_err(|position| Error::InvalidParameter { position })?;
        let mut query = sqlx::query(AssertSqlSafe(sql));
        for value in values {
            query = match value {
                None => query.bind(Option::<i64>::None),
                Some(Param::Boolean(flag)) => query.bind(flag),
                Some(Param::Number(number)) if number.fract() == 0.0 && (-BIGINT_END..BIGINT_END).contains(&number) => {
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
        Ok(query)
    }

    async fn execute(pool: &MySqlPool, sql: &str, params: Option<Params>) -> Result<u64, Error> {
        if pool.is_closed() {
            return Err(Error::Closed);
        }
        Ok(bound(sql, params)?.execute(pool).await?.rows_affected())
    }

    async fn query(pool: &MySqlPool, sql: &str, params: Option<Params>) -> Result<Vec<Row>, Error> {
        if pool.is_closed() {
            return Err(Error::Closed);
        }
        bound(sql, params)?.fetch_all(pool).await?.iter().map(read_row).collect()
    }

    async fn query_one(pool: &MySqlPool, sql: &str, params: Option<Params>) -> Result<Option<Row>, Error> {
        if pool.is_closed() {
            return Err(Error::Closed);
        }
        bound(sql, params)?.fetch_optional(pool).await?.as_ref().map(read_row).transpose()
    }

    fn read_row(row: &MySqlRow) -> Result<Row, Error> {
        let columns = row.columns();
        let mut out = Row::with_capacity(columns.len());
        for column in columns {
            let decode = |reason: String| Error::Decode { column: column.name().to_owned(), reason };
            let raw: MySqlValueRef<'_> = row.try_get_raw(column.ordinal()).map_err(|error| decode(error.to_string()))?;
            if raw.is_null() {
                continue;
            }
            let kind = classify(raw.type_info().name());
            let cell = read_cell(kind, raw).map_err(|error| decode(error.to_string()))?;
            out.insert(column.name().to_owned(), cell);
        }
        Ok(out)
    }

    fn read_cell(kind: Kind, raw: MySqlValueRef<'_>) -> Result<Cell, sqlx::error::BoxDynError> {
        Ok(match kind {
            Kind::Bool => Cell::Boolean(<bool as Decode<MySql>>::decode(raw)?),
            // a Lua number is a double: a BIGINT past 2^53 loses its low bits
            #[allow(clippy::cast_precision_loss)]
            Kind::Integer => Cell::Number(<i64 as Decode<MySql>>::decode(raw)? as f64),
            Kind::Real => Cell::Number(<f64 as Decode<MySql>>::decode(raw)?),
            Kind::Blob => Cell::Text(Bytes(<&[u8] as Decode<MySql>>::decode(raw)?.to_vec())),
            Kind::Text => Cell::Text(Bytes(<&str as Decode<MySql>>::decode(raw)?.as_bytes().to_vec())),
        })
    }

    /// A pool of connections to one MySQL database.
    #[derive(Clone, Object)]
    pub struct Database {
        pool: MySqlPool,
    }

    /// Open a pool of connections to a MySQL database, and make the first. `url` is a standard MySQL URL:
    /// `mysql://user:pass@host:3306/dbname`.
    ///
    /// ```luau
    /// local db, err = sqlx_mysql.open("not-a-url")
    /// assert(db == nil and err ~= nil and err.kind == "invalid_url")
    /// ```
    #[host("Wait")]
    pub async fn open(url: String) -> Result<Database, OpenError> {
        let options =
            MySqlConnectOptions::from_str(&url).map_err(|error| OpenError::InvalidUrl { reason: error.to_string() })?;
        // sqlx logs every statement at INFO by default, which is most of the cost of a hot path: off. The larger
        // statement cache keeps reused SQL prepared.
        let options = options
            .statement_cache_capacity(STATEMENT_CACHE_CAPACITY)
            .log_statements(LevelFilter::Off)
            .log_slow_statements(LevelFilter::Off, std::time::Duration::from_secs(1));
        match PoolOptions::<MySql>::new().connect_with(options).await {
            Ok(pool) => Ok(Database { pool }),
            Err(error) => Err(OpenError::Connect { reason: error.to_string() }),
        }
    }

    #[mlua_ir::object]
    impl Database {
        /// Run one statement; how many rows it changed.
        #[host("Wait")]
        pub async fn execute(&self, sql: String, params: Option<Params>) -> Result<u64, Error> {
            execute(&self.pool, &sql, params).await
        }

        /// Run one SELECT; every row, in order.
        #[host("Wait")]
        pub async fn query(&self, sql: String, params: Option<Params>) -> Result<Vec<Row>, Error> {
            query(&self.pool, &sql, params).await
        }

        /// Run one SELECT; its first row, or `nil` when it has none.
        #[host("Wait")]
        pub async fn query_one(&self, sql: String, params: Option<Params>) -> Result<Option<Row>, Error> {
            query_one(&self.pool, &sql, params).await
        }

        /// A statement to run many times on this database's pool. The SQL is not checked here: a mistake in it is
        /// the error of the statement's first call.
        pub fn prepare(&self, sql: String) -> Statement {
            Statement { sql: Arc::from(sql), pool: self.pool.clone() }
        }

        /// Close every connection of the pool, waiting for the ones in use. Later calls on this database and on
        /// its statements fail with `closed`; closing again does nothing.
        #[host("Wait")]
        pub async fn close(&self) {
            self.pool.close().await;
        }
    }

    /// SQL kept for many calls, made by `db:prepare(sql)`. It runs on the database's pool; sqlx keeps the
    /// statement prepared on each connection either way, so what this saves is passing the SQL text from Lua.
    #[derive(Clone, Object)]
    pub struct Statement {
        sql: Arc<str>,
        pool: MySqlPool,
    }

    #[mlua_ir::object]
    impl Statement {
        /// Run the statement; how many rows it changed.
        #[host("Wait")]
        pub async fn execute(&self, params: Option<Params>) -> Result<u64, Error> {
            execute(&self.pool, &self.sql, params).await
        }

        /// Run the SELECT; every row, in order.
        #[host("Wait")]
        pub async fn query(&self, params: Option<Params>) -> Result<Vec<Row>, Error> {
            query(&self.pool, &self.sql, params).await
        }

        /// Run the SELECT; its first row, or `nil` when it has none.
        #[host("Wait")]
        pub async fn query_one(&self, params: Option<Params>) -> Result<Option<Row>, Error> {
            query_one(&self.pool, &self.sql, params).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Kind, classify};
    use crate::make_engine;

    #[test]
    fn column_types_are_classified() {
        for (name, kind) in [
            ("BOOLEAN", Kind::Bool),
            ("TINYINT", Kind::Integer),
            ("INT UNSIGNED", Kind::Integer),
            ("BIGINT", Kind::Integer),
            ("MEDIUMINT", Kind::Integer),
            ("FLOAT", Kind::Real),
            ("DOUBLE", Kind::Real),
            ("BLOB", Kind::Blob),
            ("LONGBLOB", Kind::Blob),
            ("VARBINARY", Kind::Blob),
            ("VARCHAR", Kind::Text),
            ("TEXT", Kind::Text),
            ("DECIMAL", Kind::Text),
            ("DATETIME", Kind::Text),
            ("JSON", Kind::Text),
            ("INTERVAL", Kind::Text),
        ] {
            assert_eq!(classify(name), kind, "{name}");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_malformed_url_is_a_typed_error() {
        let lua = make_engine().unwrap();
        let (kind, op, text): (String, String, String) = lua
            .load(
                r#"
                local db, err = require("vnd:sqlx_mysql").open("not-a-url")
                assert(db == nil)
                return err.kind, err.op, tostring(err)
            "#,
            )
            .eval_async()
            .await
            .unwrap();
        assert_eq!(kind, "invalid_url");
        assert_eq!(op, "vnd:sqlx_mysql.open");
        assert!(text.starts_with("vnd:sqlx_mysql.open: invalid_url: "), "{text}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_url_that_is_not_a_string_is_refused_as_an_argument() {
        let lua = make_engine().unwrap();
        let text: String = lua
            .load(
                r#"
                local ok, refused = pcall(require("vnd:sqlx_mysql").open, 5)
                assert(not ok)
                return tostring(refused)
            "#,
            )
            .eval_async()
            .await
            .unwrap();
        assert!(text.contains("vnd:sqlx_mysql.open: argument #1 'url'"), "{text}");
    }

    /// neoc-vm's `sqlx_mysql.test.lua`, with the parameters as lists and the errors typed. `URL` is the server.
    const SERVER_SCRIPT: &str = r#"
        local sqlx_mysql = require("vnd:sqlx_mysql")

        -- open
        local db, open_err = sqlx_mysql.open(URL)
        assert(open_err == nil and db ~= nil)

        -- execute: rows affected
        db:execute("DROP TABLE IF EXISTS exec_t")
        local n, err = db:execute("CREATE TABLE exec_t (id INT PRIMARY KEY, v VARCHAR(64))")
        assert(err == nil and n == 0)
        n, err = db:execute("INSERT INTO exec_t (id, v) VALUES (?, ?)", { 1, "alice" })
        assert(err == nil and n == 1)
        assert(db:execute("INSERT INTO exec_t VALUES (2, 'a'), (3, 'b'), (4, 'a')") == 3)
        assert(db:execute("UPDATE exec_t SET v = ? WHERE v = ?", { "x", "a" }) == 2)

        -- invalid SQL
        local bad, bad_err = db:execute("NOT VALID SQL")
        assert(bad == nil and bad_err.kind == "database" and bad_err.op == "vnd:sqlx_mysql.Database:execute")
        assert(bad_err.code == "42000")

        -- a constraint
        local dup, dup_err = db:execute("INSERT INTO exec_t (id, v) VALUES (?, ?)", { 1, "again" })
        assert(dup == nil and dup_err.kind == "constraint" and dup_err.violation == "unique")

        -- a position without a value binds NULL
        db:execute("DROP TABLE IF EXISTS exec_null")
        db:execute("CREATE TABLE exec_null (v BIGINT, id INT PRIMARY KEY)")
        assert(db:execute("INSERT INTO exec_null (v, id) VALUES (?, ?)", { nil, 1 }) == 1)
        local null_row = assert(db:query_one("SELECT id, v FROM exec_null WHERE id = 1"))
        assert(null_row.id == 1 and null_row.v == nil)
        -- a trailing nil is not seen: the server counts one parameter too few
        local short, short_err = db:execute("INSERT INTO exec_null (id, v) VALUES (?, ?)", { 2, nil })
        assert(short == nil and short_err ~= nil)
        local zero, zero_err = db:execute("SELECT ?", { [0] = 1 })
        assert(zero == nil and zero_err.kind == "invalid_parameter" and zero_err.position == 0)

        -- query
        local rows, query_err = db:query("SELECT id, v FROM exec_t ORDER BY id")
        assert(query_err == nil and #rows == 4)
        assert(rows[1].id == 1 and rows[1].v == "alice")
        assert(rows[2].id == 2 and rows[2].v == "x")
        assert(#assert(db:query("SELECT * FROM exec_t WHERE id > 99")) == 0)
        local where = assert(db:query("SELECT v FROM exec_t WHERE v = ?", { "b" }))
        assert(#where == 1 and where[1].v == "b")

        -- the types of values
        db:execute("DROP TABLE IF EXISTS q_types")
        db:execute("CREATE TABLE q_types (id INT PRIMARY KEY, x DOUBLE, b BLOB, f BOOLEAN, d DECIMAL(6,2), big BIGINT)")
        assert(db:execute("INSERT INTO q_types VALUES (1, ?, ?, ?, 12.50, ?)", { 3.14, "\0\1\2\254\255", true, 2 ^ 40 }) == 1)
        local typed = assert(db:query_one("SELECT x, b, f, d, big FROM q_types WHERE id = 1"))
        assert(typed.x == 3.14 and typed.b == "\0\1\2\254\255" and typed.f == true)
        assert(typed.d == "12.50" and typed.big == 2 ^ 40)

        -- query_one
        local row, one_err = db:query_one("SELECT id, v FROM exec_t WHERE id = ?", { 1 })
        assert(one_err == nil and row.id == 1 and row.v == "alice")
        local none, none_err = db:query_one("SELECT * FROM exec_t WHERE id = ?", { 999 })
        assert(none == nil and none_err == nil)

        -- prepare
        db:execute("DROP TABLE IF EXISTS p_t")
        db:execute("CREATE TABLE p_t (id INT PRIMARY KEY, v VARCHAR(64))")
        local ins = db:prepare("INSERT INTO p_t (id, v) VALUES (?, ?)")
        assert(ins:execute({ 1, "a" }) == 1)
        assert(ins:execute({ 2, "b" }) == 1)
        assert(ins:execute({ 3, "a" }) == 1)
        local sel = db:prepare("SELECT id, v FROM p_t WHERE v = ? ORDER BY id")
        local many = assert(sel:query({ "a" }))
        assert(#many == 2 and many[1].id == 1 and many[2].id == 3)
        assert(assert(sel:query_one({ "b" })).id == 2)
        local _, stmt_err = db:prepare("NOT VALID SQL"):execute()
        assert(stmt_err.kind == "database" and stmt_err.op == "vnd:sqlx_mysql.Statement:execute")

        -- close: idempotent, and everything after it fails
        db:close()
        db:close()
        local closed, closed_err = db:execute("SELECT 1")
        assert(closed == nil and closed_err.kind == "closed")
        assert(tostring(closed_err) == "vnd:sqlx_mysql.Database:execute: closed: database is closed")
        local _, stmt_closed = sel:query({ "a" })
        assert(stmt_closed.kind == "closed")
    "#;

    /// Against a real server: `NEOC_TEST_MYSQL_URL=mysql://root:pw@127.0.0.1:3306/test cargo test -p neoc-vm2
    /// sqlx_mysql -- --ignored`. The script creates and drops its own tables (`exec_t`, `exec_null`, `q_types`,
    /// `p_t`).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "needs a MySQL server: set NEOC_TEST_MYSQL_URL"]
    async fn against_a_server() {
        let url = std::env::var("NEOC_TEST_MYSQL_URL").expect("NEOC_TEST_MYSQL_URL is the server to test against");
        let lua = make_engine().unwrap();
        lua.globals().set("URL", url).unwrap();
        lua.load(SERVER_SCRIPT).exec_async().await.unwrap();
    }
}
