# neoc-autosql

Generate a type-safe neoc data-access layer from a SQL schema and a set of
single-query files — sqlc, but SQL → neoc. See [`ASSUMPTIONS.md`](ASSUMPTIONS.md)
for the modelling choices and current limitations.

A compiler-domain tool: it parses SQL with `sqlparser` and emits **`.neoc`
source** that references no runtime module. The only seam to a database is a
user-written `impl SqlExecutor` (declared through `ext`). autosql's job ends at
producing valid neoc — building that neoc to a target is `neoc build`'s concern.

## Use

`neoc autosql` is convention-driven — it discovers the project from the working
directory and reads its SQL from the fixed layout:

```
src/main/sql/ddl/{n}-{description}.sql   -- migrations, applied in ascending {n}
src/main/sql/dml/{name}.sql              -- one query per file
```

```sh
neoc autosql            # writes src/main/neoc/autosql/
neoc autosql --dialect postgres
```

## What it emits

A set of files under `src/main/neoc/autosql/`:

- **`ports.neoc`** — the shared contract: the `SqlValue` enum, the
  `SqlExecutor` trait, and the `bind_opt_*` / `decode_*` row helpers. The one
  file every other generated file imports.
- **`migrate.neoc`** — `pub fn migrate<E: SqlExecutor>(db: E)` runs every DDL
  statement, in migration order, through `SqlExecutor::execute`.
- **`<query>.neoc`** per `dml` file — its own row `struct`, its row decoder, and
  the typed query function (`fn <name><E: SqlExecutor>(db: E, …)`). A `SELECT`
  returns `Result<[Row], string>`; an `INSERT`/`UPDATE`/`DELETE` returns
  `Result<number, string>` (rows affected).

Each query file imports only `autosql::ports`.

You implement `SqlExecutor` for your runtime via `ext`, and that implementation
is the only code that touches a database:

```neoc
// your file — the only place a runtime appears
use autosql::ports::SqlValue;
use autosql::ports::SqlExecutor;

ext fn my_query(sql: string, params: [SqlValue]) -> Result<[[SqlValue]], string>;
struct MyDb;
impl autosql::ports::SqlExecutor for MyDb {
    fn query(self, sql: string, params: [SqlValue]) -> Result<[[SqlValue]], string> {
        my_query(sql, params)
    }
    // … execute …
}
```

## Return shapes

| Statement | Return type |
| --- | --- |
| `SELECT …`  | `Result<[Row], string>` |
| `INSERT` / `UPDATE` / `DELETE` | `Result<number, string>` (rows affected) |
