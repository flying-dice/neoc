//! SQLite: `vnd:rusqlite` (the `rusqlite` crate, with its bundled libsqlite3).
//!
//! ```lua
//! local rusqlite = require("vnd:rusqlite")
//! local db = assert(rusqlite.open(":memory:"))
//! db:execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)")
//! local n, err = db:execute("INSERT INTO users (name) VALUES (?)", "alice")
//! local rows = assert(db:query("SELECT id, name FROM users"))
//! -- rows[1].id == 1, rows[1].name == "alice"
//! local row = db:query_one("SELECT * FROM users WHERE id = ?", 1)
//!
//! -- For hot paths, prepare once and reuse:
//! local insert = db:prepare("INSERT INTO users (name) VALUES (?)")
//! insert:execute("alice")
//! insert:execute("bob")
//! db:close()
//! ```
//!
//! One statement per call: `execute`, `query` and `query_one` (on a `Database` and on a `Statement`) take exactly
//! one SQL statement. A second statement is the error `multiple_statements`, and SQL that is empty or only a
//! comment is `empty_sql`. There is no `execute_batch`: run a script one statement per call.
//!
//! Parameters: the arguments after the SQL (`db:execute(sql, a, b)`; every argument of `stmt:execute(a, b)`) are
//! the values for the statement's placeholders, in order. A `nil` binds SQL `NULL`, the last one too. As many
//! values as the statement has placeholders, or the error is `parameter_count`. A boolean binds as 0 or 1, a
//! whole number as INTEGER, any other number as REAL, a string as TEXT when it is UTF-8 and as BLOB otherwise.
//!
//! Rows: a row is a table from column name to value. INTEGER and REAL come back as numbers, TEXT and BLOB as
//! strings, `NULL` as `nil` (the key is absent).
//!
//! Why sync: libsqlite3 is a synchronous C library, and every call returns without yielding to IO (a file-backed
//! database hits the kernel page cache). An async wrapper would add the future, the coroutine yield and the
//! scheduler's work to a query that takes less than a microsecond. The cost: a slow query (a giant SELECT, an
//! fsync-heavy commit on a slow disk) holds the calling worker for as long as it runs.
//!
//! Connection model: a `Database` is one `rusqlite::Connection` behind a `parking_lot::Mutex` (the connection is
//! `Send` but not `Sync`, and userdata must be both). Every method is sync and coroutines of one engine never run
//! at once, so the lock is uncontended in practice: it is held for one statement and never across a yield.
//! SQLite's own per-connection mutex is off (`OPEN_NO_MUTEX`) because this one serialises access already.
//! Statements go through rusqlite's `prepare_cached`, so the same SQL is parsed once.
//!
//! A `Statement` holds its SQL and shares the database's connection. Its calls take the same `prepare_cached`
//! path as `db:execute(sql, ...)`; what it saves is passing the SQL text from Lua on each call.
//!
//! Ported from the old VM's `vnd:rusqlite`. What changed on purpose:
//! - A parameter that is not a boolean, a number or a string (and SQL that is not a string) is refused as an
//!   argument; it was a returned error.
//! - A failure is a typed error: `OpenError` (`cannot_open`) and `Error` (`closed`, `multiple_statements`,
//!   `empty_sql`, `parameter_count`, `constraint`, `busy`, `sql`, `other`).
//! - A statement's errors name `vnd:rusqlite.Statement:execute` as their `op` (it was `rusqlite.statement.execute`).

#[mlua_ir::module("vnd:rusqlite")]
pub mod rusqlite {
    use std::collections::HashMap;
    use std::sync::Arc;

    use ::rusqlite::types::{Value as SqlValue, ValueRef};
    use ::rusqlite::{ErrorCode, OpenFlags};
    use mlua_ir::{Bytes, LuaError, Object, OneOf, Variadic};
    use parking_lot::{MappedMutexGuard, Mutex, MutexGuard};
    use serde::Serialize;

    /// A value bound to a placeholder. A boolean binds as 0 or 1, a whole number as INTEGER, any other number as
    /// REAL, a string as TEXT when it is UTF-8 and as BLOB otherwise.
    #[derive(Debug, Clone, OneOf)]
    pub enum Param {
        /// 0 or 1.
        Boolean(bool),
        /// INTEGER when whole, REAL otherwise.
        Number(f64),
        /// TEXT when UTF-8, BLOB otherwise.
        Text(Bytes),
    }

    /// A value of a row. INTEGER and REAL are numbers, TEXT and BLOB are strings.
    #[derive(Debug, Clone, OneOf)]
    pub enum Cell {
        /// An INTEGER or REAL column.
        Number(f64),
        /// A TEXT or BLOB column, byte for byte.
        Text(Bytes),
    }

    /// A row: column name to value. A `NULL` column is absent (`nil`).
    pub type Row = HashMap<String, Cell>;

    /// Why a database could not be opened.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum OpenError {
        /// The file cannot be opened or created, or is not a database path SQLite accepts.
        #[error("cannot open {path}: {reason}")]
        CannotOpen {
            /// The path as given.
            path: String,
            /// What SQLite said.
            reason: String,
        },
    }

    /// Why a statement failed.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum Error {
        /// `close` was called on this database.
        #[error("database is closed")]
        Closed,
        /// The SQL holds more than one statement. Nothing was run.
        #[error("multiple statements provided: run one statement per call")]
        MultipleStatements,
        /// The SQL is empty or only a comment.
        #[error("SQL is empty or comment only")]
        EmptySql,
        /// The call gave more or fewer values than the statement has placeholders. Nothing was run.
        #[error("{given} values were given, the statement has {expected} placeholders")]
        ParameterCount {
            /// How many values the call gave, `nil`s included.
            given: u32,
            /// How many placeholders the statement has.
            expected: u32,
        },
        /// A constraint refused the change: UNIQUE, NOT NULL, CHECK, FOREIGN KEY, PRIMARY KEY.
        #[error("{reason}")]
        Constraint {
            /// What SQLite said, naming the constraint.
            reason: String,
            /// SQLite's extended result code (2067 is `SQLITE_CONSTRAINT_UNIQUE`).
            code: i32,
        },
        /// The database or a table is locked by another connection.
        #[error("{reason}")]
        Busy {
            /// What SQLite said.
            reason: String,
        },
        /// SQLite refused the statement: a syntax error, a missing table, a full disk.
        #[error("{reason}")]
        Sql {
            /// What SQLite said.
            reason: String,
            /// SQLite's extended result code.
            code: i32,
        },
        /// Anything else: `execute` given a statement that returns rows, a value SQLite cannot hold.
        #[error("{reason}")]
        Other {
            /// What went wrong.
            reason: String,
        },
    }

    impl From<::rusqlite::Error> for Error {
        fn from(error: ::rusqlite::Error) -> Self {
            let reason = error.to_string();
            match error {
                ::rusqlite::Error::MultipleStatement => Error::MultipleStatements,
                ::rusqlite::Error::Empty => Error::EmptySql,
                ::rusqlite::Error::SqliteFailure(failure, _) => match failure.code {
                    ErrorCode::ConstraintViolation => {
                        Error::Constraint { reason, code: failure.extended_code }
                    }
                    ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => Error::Busy { reason },
                    _ => Error::Sql { reason, code: failure.extended_code },
                },
                ::rusqlite::Error::SqlInputError { error: failure, .. } => {
                    Error::Sql { reason, code: failure.extended_code }
                }
                _ => Error::Other { reason },
            }
        }
    }

    /// A rusqlite connection behind a mutex. `None` once `close` has dropped it.
    struct SharedConn(Mutex<Option<::rusqlite::Connection>>);

    impl SharedConn {
        /// Lock the connection, or report that the database was closed.
        fn lock(&self) -> Result<MappedMutexGuard<'_, ::rusqlite::Connection>, Error> {
            MutexGuard::try_map(self.0.lock(), Option::as_mut).map_err(|_| Error::Closed)
        }

        /// Drop the connection (rusqlite closes it); later calls report `closed`.
        fn close(&self) {
            drop(self.0.lock().take());
        }

        fn execute(&self, sql: &str, params: Vec<Option<Param>>) -> Result<u64, Error> {
            let conn = self.lock()?;
            let mut stmt = conn.prepare_cached(sql)?;
            let values = bind(&stmt, params)?;
            let changed = stmt.execute(::rusqlite::params_from_iter(values))?;
            Ok(changed as u64)
        }

        fn query(&self, sql: &str, params: Vec<Option<Param>>) -> Result<Vec<Row>, Error> {
            let conn = self.lock()?;
            let mut stmt = conn.prepare_cached(sql)?;
            let names = column_names(&stmt);
            let values = bind(&stmt, params)?;
            let mut rows = stmt.query(::rusqlite::params_from_iter(values))?;
            let mut out = Vec::new();
            while let Some(row) = rows.next()? {
                out.push(read_row(row, &names)?);
            }
            Ok(out)
        }

        fn query_one(&self, sql: &str, params: Vec<Option<Param>>) -> Result<Option<Row>, Error> {
            let conn = self.lock()?;
            let mut stmt = conn.prepare_cached(sql)?;
            let names = column_names(&stmt);
            let values = bind(&stmt, params)?;
            let mut rows = stmt.query(::rusqlite::params_from_iter(values))?;
            Ok(rows.next()?.map(|row| read_row(row, &names)).transpose()?)
        }
    }

    /// One value for each of the statement's placeholders, `NULL` for a `nil`.
    fn bind(stmt: &::rusqlite::Statement<'_>, params: Vec<Option<Param>>) -> Result<Vec<SqlValue>, Error> {
        let expected = stmt.parameter_count();
        if params.len() != expected {
            let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
            return Err(Error::ParameterCount { given: count(params.len()), expected: count(expected) });
        }
        Ok(params.into_iter().map(|param| param.map_or(SqlValue::Null, SqlValue::from)).collect())
    }

    /// 2^63: the first whole number past what an INTEGER holds.
    const INTEGER_END: f64 = 9_223_372_036_854_775_808.0;

    impl From<Param> for SqlValue {
        fn from(param: Param) -> Self {
            match param {
                Param::Boolean(flag) => SqlValue::Integer(i64::from(flag)),
                Param::Number(number)
                    if number.fract() == 0.0 && (-INTEGER_END..INTEGER_END).contains(&number) =>
                {
                    // whole and in range: the cast is exact
                    #[allow(clippy::cast_possible_truncation)]
                    SqlValue::Integer(number as i64)
                }
                Param::Number(number) => SqlValue::Real(number),
                // Lua strings are bytes: what is not UTF-8 is a blob, and reads back as the same string
                Param::Text(Bytes(bytes)) => match String::from_utf8(bytes) {
                    Ok(text) => SqlValue::Text(text),
                    Err(not_text) => SqlValue::Blob(not_text.into_bytes()),
                },
            }
        }
    }

    fn column_names(stmt: &::rusqlite::Statement<'_>) -> Vec<String> {
        (0..stmt.column_count()).map(|index| stmt.column_name(index).unwrap_or("").to_owned()).collect()
    }

    fn read_row(row: &::rusqlite::Row<'_>, names: &[String]) -> ::rusqlite::Result<Row> {
        let mut out = Row::with_capacity(names.len());
        for (index, name) in names.iter().enumerate() {
            let cell = match row.get_ref(index)? {
                ValueRef::Null => continue,
                // a Lua number is a double: an INTEGER past 2^53 loses its low bits
                #[allow(clippy::cast_precision_loss)]
                ValueRef::Integer(number) => Cell::Number(number as f64),
                ValueRef::Real(number) => Cell::Number(number),
                ValueRef::Text(text) => {
                    Cell::Text(Bytes(String::from_utf8_lossy(text).into_owned().into_bytes()))
                }
                ValueRef::Blob(bytes) => Cell::Text(Bytes(bytes.to_vec())),
            };
            out.insert(name.clone(), cell);
        }
        Ok(out)
    }

    /// An open SQLite database: one connection.
    #[derive(Clone, Object)]
    pub struct Database {
        /// Shared with the statements `prepare` makes; the connection inside is gone once `close` ran.
        conn: Arc<SharedConn>,
    }

    /// Open a SQLite database file, creating it when it does not exist. `":memory:"` is a database in memory,
    /// gone when it is closed; a `file:` URI is read as one.
    ///
    /// ```luau
    /// local db = assert(rusqlite.open(":memory:"))
    /// assert(db:execute("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)"))
    /// assert(db:execute("INSERT INTO t (v) VALUES (?)", "hello") == 1)
    /// local row = assert(db:query_one("SELECT id, v FROM t WHERE v = ?", "hello"))
    /// assert(row.id == 1 and row.v == "hello")
    /// db:close()
    /// ```
    pub fn open(path: String) -> Result<Database, OpenError> {
        // OPEN_NO_MUTEX: `SharedConn` serialises access, SQLite's own per-connection mutex would be the same
        // work twice.
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI;
        match ::rusqlite::Connection::open_with_flags(&path, flags) {
            Ok(conn) => Ok(Database { conn: Arc::new(SharedConn(Mutex::new(Some(conn)))) }),
            Err(error) => Err(OpenError::CannotOpen { path, reason: error.to_string() }),
        }
    }

    #[mlua_ir::object]
    impl Database {
        /// Run one statement; how many rows it changed (0 for a statement that changes none, such as
        /// `CREATE TABLE`).
        pub fn execute(&self, sql: String, params: Variadic<Option<Param>>) -> Result<u64, Error> {
            self.conn.execute(&sql, params.0)
        }

        /// Run one SELECT; every row, in order.
        pub fn query(&self, sql: String, params: Variadic<Option<Param>>) -> Result<Vec<Row>, Error> {
            self.conn.query(&sql, params.0)
        }

        /// Run one SELECT; its first row, or `nil` when it has none.
        pub fn query_one(&self, sql: String, params: Variadic<Option<Param>>) -> Result<Option<Row>, Error> {
            self.conn.query_one(&sql, params.0)
        }

        /// A statement to run many times on this database's connection. The SQL is not checked here: a mistake
        /// in it is the error of the statement's first call.
        ///
        /// ```luau
        /// local db = assert(rusqlite.open(":memory:"))
        /// assert(db:execute("CREATE TABLE t (v TEXT)"))
        /// local insert = db:prepare("INSERT INTO t (v) VALUES (?)")
        /// assert(insert:execute("a") == 1)
        /// assert(insert:execute("b") == 1)
        /// ```
        pub fn prepare(&self, sql: String) -> Statement {
            Statement { sql: Arc::from(sql), conn: self.conn.clone() }
        }

        /// Close the connection. Later calls on this database and on its statements fail with `closed`; closing
        /// again does nothing.
        pub fn close(&self) {
            self.conn.close();
        }
    }

    /// SQL kept for many calls, made by `db:prepare(sql)`. It runs on the database's connection.
    #[derive(Clone, Object)]
    pub struct Statement {
        sql: Arc<str>,
        conn: Arc<SharedConn>,
    }

    #[mlua_ir::object]
    impl Statement {
        /// Run the statement; how many rows it changed.
        pub fn execute(&self, params: Variadic<Option<Param>>) -> Result<u64, Error> {
            self.conn.execute(&self.sql, params.0)
        }

        /// Run the SELECT; every row, in order.
        pub fn query(&self, params: Variadic<Option<Param>>) -> Result<Vec<Row>, Error> {
            self.conn.query(&self.sql, params.0)
        }

        /// Run the SELECT; its first row, or `nil` when it has none.
        pub fn query_one(&self, params: Variadic<Option<Param>>) -> Result<Option<Row>, Error> {
            self.conn.query_one(&self.sql, params.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    use crate::make_engine;

    const PRELUDE: &str = "local rusqlite = require(\"vnd:rusqlite\")\n";

    fn engine() -> Lua {
        make_engine().unwrap()
    }

    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        engine().load(format!("{PRELUDE}{code}")).eval().unwrap()
    }

    /// Run a script whose asserts are the test.
    fn run(code: &str) {
        engine().load(format!("{PRELUDE}{code}")).exec().unwrap();
    }

    #[test]
    fn memory_database_opens() {
        run(r#"
            local db, err = rusqlite.open(":memory:")
            assert(err == nil and db ~= nil)
            db:close()
        "#);
    }

    #[test]
    fn a_file_path_opens_and_is_created() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sqlite_create.db");
        let lua = engine();
        lua.globals().set("PATH", path.to_str().unwrap()).unwrap();
        lua.load(
            r#"
            local rusqlite = require("vnd:rusqlite")
            local db = assert(rusqlite.open(PATH))
            assert(db:execute("CREATE TABLE IF NOT EXISTS t (id INTEGER)") == 0)
            assert(db:execute("INSERT INTO t VALUES (7)") == 1)
            db:close()
            local again = assert(rusqlite.open(PATH))
            assert(assert(again:query_one("SELECT id FROM t")).id == 7)
            again:close()
        "#,
        )
        .exec()
        .unwrap();
        assert!(path.exists());
    }

    #[test]
    fn a_path_that_cannot_be_opened_is_a_typed_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing").join("x.db");
        let lua = engine();
        lua.globals().set("PATH", path.to_str().unwrap()).unwrap();
        let (kind, same_path, op): (String, bool, String) = lua
            .load(
                r#"
                local db, err = require("vnd:rusqlite").open(PATH)
                assert(db == nil)
                return err.kind, err.path == PATH, err.op
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(kind, "cannot_open");
        assert!(same_path);
        assert_eq!(op, "vnd:rusqlite.open");
    }

    #[test]
    fn execute_returns_rows_affected() {
        let (created, inserted, updated): (u64, u64, u64) = eval(
            r#"
            local db = assert(rusqlite.open(":memory:"))
            local created = assert(db:execute("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)"))
            local inserted = assert(db:execute("INSERT INTO t (v) VALUES (?)", "a"))
            db:execute("INSERT INTO t (v) VALUES (?)", "b")
            db:execute("INSERT INTO t (v) VALUES (?)", "a")
            local updated = assert(db:execute("UPDATE t SET v = ? WHERE v = ?", "x", "a"))
            return created, inserted, updated
        "#,
        );
        assert_eq!((created, inserted, updated), (0, 1, 2));
    }

    #[test]
    fn invalid_sql_is_a_typed_error() {
        let (kind, text, code): (String, String, i32) = eval(
            r#"
            local db = assert(rusqlite.open(":memory:"))
            local n, err = db:execute("NOT VALID SQL")
            assert(n == nil)
            return err.kind, tostring(err), err.code
        "#,
        );
        assert_eq!(kind, "sql");
        assert!(text.starts_with("vnd:rusqlite.Database:execute: sql: "), "{text}");
        assert!(text.contains("syntax error"), "{text}");
        assert_eq!(code, 1);
    }

    #[test]
    fn a_second_statement_is_refused_and_nothing_runs() {
        let (kind, tables, query_kind, one_kind): (String, usize, String, String) = eval(
            r#"
            local db = assert(rusqlite.open(":memory:"))
            local n, err = db:execute("CREATE TABLE a (x INTEGER); CREATE TABLE b (x INTEGER)")
            assert(n == nil)
            local tables = #assert(db:query("SELECT name FROM sqlite_master WHERE type = 'table'"))
            local rows, query_err = db:query("SELECT 1 AS a; SELECT 2 AS a")
            local row, one_err = db:query_one("SELECT 1 AS a; SELECT 2 AS a")
            assert(rows == nil and row == nil)
            return err.kind, tables, query_err.kind, one_err.kind
        "#,
        );
        assert_eq!(kind, "multiple_statements");
        assert_eq!(tables, 0);
        assert_eq!(query_kind, "multiple_statements");
        assert_eq!(one_kind, "multiple_statements");
    }

    #[test]
    fn separate_calls_run_each_statement() {
        let tables: usize = eval(
            r#"
            local db = assert(rusqlite.open(":memory:"))
            assert(db:execute("CREATE TABLE a (x INTEGER)") == 0)
            assert(db:execute("CREATE TABLE b (x INTEGER)") == 0)
            return #assert(db:query("SELECT name FROM sqlite_master WHERE type = 'table'"))
        "#,
        );
        assert_eq!(tables, 2);
    }

    #[test]
    fn empty_or_comment_only_sql_is_an_error() {
        let (empty, comment): (String, String) = eval(
            r#"
            local db = assert(rusqlite.open(":memory:"))
            local n, err = db:execute("")
            local n2, err2 = db:execute("-- just a comment")
            assert(n == nil and n2 == nil)
            assert(err2.kind == "empty_sql")
            return tostring(err), tostring(err2)
        "#,
        );
        assert_eq!(empty, "vnd:rusqlite.Database:execute: empty_sql: SQL is empty or comment only");
        assert_eq!(comment, empty);
    }

    #[test]
    fn nil_binds_null() {
        run(r#"
            local db = assert(rusqlite.open(":memory:"))
            db:execute("CREATE TABLE t (a, b, c)")
            -- the only value, a trailing nil, a leading and a trailing nil, a nil in the middle
            assert(db:execute("INSERT INTO t (a) VALUES (?)", nil) == 1)
            assert(db:execute("INSERT INTO t VALUES (?, ?, ?)", 1, 2, nil) == 1)
            assert(db:execute("INSERT INTO t VALUES (?, ?, ?)", nil, 2, nil) == 1)
            assert(db:execute("INSERT INTO t VALUES (?, ?, ?)", 1, nil, 3) == 1)
            local rows = assert(db:query("SELECT a, b, c FROM t ORDER BY rowid"))
            assert(next(rows[1]) == nil)
            assert(rows[2].a == 1 and rows[2].b == 2 and rows[2].c == nil)
            assert(rows[3].a == nil and rows[3].b == 2 and rows[3].c == nil)
            assert(rows[4].a == 1 and rows[4].b == nil and rows[4].c == 3)
            -- a statement binds the same way
            local insert = db:prepare("INSERT INTO t VALUES (?, ?, ?)")
            assert(insert:execute(nil, nil, nil) == 1)
            assert(assert(db:query_one("SELECT count(*) AS n FROM t WHERE a IS NULL AND b IS NULL AND c IS NULL")).n == 2)
            assert(assert(db:query_one("SELECT ? IS NULL AS yes", nil)).yes == 1)
        "#);
    }

    #[test]
    fn a_wrong_number_of_values_is_an_error() {
        let (kind, given, expected, text): (String, u32, u32, String) = eval(
            r#"
            local db = assert(rusqlite.open(":memory:"))
            db:execute("CREATE TABLE t (a, b)")
            local n, err = db:execute("INSERT INTO t VALUES (?, ?)", "a", "b", nil)
            assert(n == nil)
            local few, few_err = db:execute("INSERT INTO t VALUES (?, ?)", "a")
            assert(few == nil and few_err.kind == "parameter_count" and few_err.given == 1 and few_err.expected == 2)
            local none, none_err = db:prepare("INSERT INTO t VALUES (?, ?)"):execute()
            assert(none == nil and none_err.kind == "parameter_count" and none_err.given == 0)
            assert(#assert(db:query("SELECT a FROM t")) == 0)
            return err.kind, err.given, err.expected, tostring(err)
        "#,
        );
        assert_eq!((kind.as_str(), given, expected), ("parameter_count", 3, 2));
        assert_eq!(
            text,
            "vnd:rusqlite.Database:execute: parameter_count: 3 values were given, the statement has 2 placeholders"
        );
    }

    #[test]
    fn a_parameter_of_another_type_is_refused_as_an_argument() {
        let (text, second): (String, String) = eval(
            r#"
            local db = assert(rusqlite.open(":memory:"))
            local ok, refused = pcall(db.execute, db, "SELECT ?", {})
            assert(not ok)
            local ok2, refused2 = pcall(db.query, db, "SELECT ?, ?", 1, print)
            local ok3 = pcall(db.execute, db, 5)
            assert(not ok2 and not ok3)
            return tostring(refused), tostring(refused2)
        "#,
        );
        assert!(text.contains("vnd:rusqlite.Database:execute: argument #2 'params'"), "{text}");
        assert!(second.contains("vnd:rusqlite.Database:query: argument #3 'params'"), "{second}");
    }

    #[test]
    fn query_returns_rows_in_order() {
        run(r#"
            local db = assert(rusqlite.open(":memory:"))
            db:execute("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
            db:execute("INSERT INTO t (v) VALUES (?)", "a")
            db:execute("INSERT INTO t (v) VALUES (?)", "b")
            local rows, err = db:query("SELECT id, v FROM t ORDER BY id")
            assert(err == nil and #rows == 2)
            assert(rows[1].id == 1 and rows[1].v == "a")
            assert(rows[2].id == 2 and rows[2].v == "b")
            local where = assert(db:query("SELECT v FROM t WHERE v = ?", "b"))
            assert(#where == 1 and where[1].v == "b")
            local none = assert(db:query("SELECT * FROM t WHERE id > 99"))
            assert(#none == 0)
        "#);
    }

    #[test]
    fn values_keep_their_types() {
        run(r#"
            local db = assert(rusqlite.open(":memory:"))
            db:execute("CREATE TABLE t (i, r, s, b, f, big)")
            db:execute("INSERT INTO t VALUES (?, ?, ?, ?, ?, ?)", 42, 3.14, "text", "\0\1\2\254\255", true, 2 ^ 70)
            local row = assert(db:query_one(
                "SELECT i, r, s, b, f, big, typeof(i) AS ti, typeof(r) AS tr, typeof(s) AS ts, typeof(b) AS tb, "
                    .. "typeof(f) AS tf, typeof(big) AS tbig FROM t"
            ))
            assert(row.i == 42 and row.ti == "integer")
            assert(row.r == 3.14 and row.tr == "real")
            assert(row.s == "text" and row.ts == "text")
            assert(row.b == "\0\1\2\254\255" and row.tb == "blob")
            assert(row.f == 1 and row.tf == "integer")
            assert(row.big == 2 ^ 70 and row.tbig == "real")
        "#);
    }

    #[test]
    fn query_one_returns_the_first_row_or_nil() {
        run(r#"
            local db = assert(rusqlite.open(":memory:"))
            db:execute("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
            db:execute("INSERT INTO t (v) VALUES (?)", "alice")
            db:execute("INSERT INTO t (v) VALUES (?)", "bob")
            local row, err = db:query_one("SELECT id, v FROM t WHERE id = ?", 1)
            assert(err == nil and row.id == 1 and row.v == "alice")
            local first = assert(db:query_one("SELECT v FROM t ORDER BY id DESC"))
            assert(first.v == "bob")
            local none, none_err = db:query_one("SELECT * FROM t WHERE id = ?", 999)
            assert(none == nil and none_err == nil)
        "#);
    }

    #[test]
    fn a_prepared_statement_runs_many_times() {
        run(r#"
            local db = assert(rusqlite.open(":memory:"))
            db:execute("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
            local ins = db:prepare("INSERT INTO t (v) VALUES (?)")
            assert(ins:execute("a") == 1)
            assert(ins:execute("b") == 1)
            local rows = assert(db:query("SELECT v FROM t ORDER BY id"))
            assert(rows[1].v == "a" and rows[2].v == "b")
            local sel = db:prepare("SELECT id, v FROM t WHERE v = ?")
            local row = assert(sel:query_one("b"))
            assert(row.id == 2 and row.v == "b")
            local many = assert(sel:query("a"))
            assert(#many == 1 and many[1].v == "a")
            assert(sel:query_one("zzz") == nil)
        "#);
    }

    #[test]
    fn a_prepared_statement_reports_its_own_op() {
        let (kind, op): (String, String) = eval(
            r#"
            local db = assert(rusqlite.open(":memory:"))
            local bad = db:prepare("NOT VALID SQL")
            local n, err = bad:execute()
            assert(n == nil)
            return err.kind, err.op
        "#,
        );
        assert_eq!(kind, "sql");
        assert_eq!(op, "vnd:rusqlite.Statement:execute");
    }

    #[test]
    fn a_constraint_violation_is_its_own_kind() {
        let (kind, code, text): (String, i32, String) = eval(
            r#"
            local db = assert(rusqlite.open(":memory:"))
            db:execute("CREATE TABLE t (v TEXT UNIQUE)")
            assert(db:execute("INSERT INTO t VALUES ('a')") == 1)
            local n, err = db:execute("INSERT INTO t VALUES ('a')")
            assert(n == nil)
            return err.kind, err.code, err.message
        "#,
        );
        assert_eq!(kind, "constraint");
        assert_eq!(code, 2067);
        assert!(text.contains("UNIQUE constraint failed: t.v"), "{text}");
    }

    #[test]
    fn execute_of_a_select_is_an_error() {
        let kind: String = eval(
            r#"
            local db = assert(rusqlite.open(":memory:"))
            local n, err = db:execute("SELECT 1")
            assert(n == nil)
            return err.kind
        "#,
        );
        assert_eq!(kind, "other");
    }

    #[test]
    fn calls_after_close_fail_and_close_is_idempotent() {
        let (text, statement_kind): (String, String) = eval(
            r#"
            local db = assert(rusqlite.open(":memory:"))
            local stmt = db:prepare("SELECT 1 AS one")
            db:close()
            db:close()
            local n, err = db:execute("CREATE TABLE t (v)")
            local rows, query_err = db:query("SELECT 1")
            local row, one_err = db:query_one("SELECT 1")
            assert(n == nil and rows == nil and row == nil)
            assert(query_err.kind == "closed" and one_err.kind == "closed")
            local _, statement_err = stmt:query()
            return tostring(err), statement_err.kind
        "#,
        );
        assert_eq!(text, "vnd:rusqlite.Database:execute: closed: database is closed");
        assert_eq!(statement_kind, "closed");
    }

    #[test]
    fn two_columns_of_one_name_keep_the_last() {
        let v: i64 = eval(
            r#"
            local db = assert(rusqlite.open(":memory:"))
            return assert(db:query_one("SELECT 1 AS a, 2 AS a")).a
        "#,
        );
        assert_eq!(v, 2);
    }
}
