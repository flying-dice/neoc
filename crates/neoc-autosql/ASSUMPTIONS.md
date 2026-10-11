# neoc-autosql — assumptions & limitations

The design was aligned with the CODEOWNER (sqlc-style input · `SqlValue` enum
seam · dialect-agnostic core · *language* feature, not a runtime feature —
couples to no `std:`/`lib:`/`vnd:` module). This document records the modelling
choices and the limitations that remain.

autosql's job is **SQL → valid `.neoc`**. It has nothing to do with how that
neoc lowers to a target (Lua, JS, …) — that is the codegen backends' concern.
This document is about the neoc autosql produces.

## What it does

- **SQL → valid neoc.** A schema + per-file queries generate structs, generic
  query functions, and pure-neoc decoders. The output parses, resolves its
  imports, and type-checks with no diagnostics (`tests/valid_neoc.rs`).
- **The executor seam is expressible.** A generic function calling a method
  through an `E: SqlExecutor` bound is valid neoc — the "linchpin" from the
  design discussion holds (STATIC §96).
- **End-to-end CLI.** `neoc autosql` discovers the project and writes
  `src/main/neoc/autosql/`.

## Output is a hexagonal `autosql/` directory

The data-access layer is emitted as several `.neoc` files under `autosql/`:

- **`ports.neoc`** — the one shared file: the `SqlValue` enum, the
  `SqlExecutor` trait, and every `bind_opt_*` / `decode_*` helper.
- **`migrate.neoc`** — runs the DDL through `SqlExecutor::execute`.
- **one file per query** — its own row struct, decoder, and query function,
  importing only `autosql::ports`.

- **One contract for the whole project.** Because the contract lives in
  `ports.neoc` rather than being inlined per file, a single `impl SqlExecutor`
  serves every query — no per-module `SqlValue` duplication.

## Input-model assumptions

- **Convention, not annotations.** Migrations live in
  `src/main/sql/ddl/{n}-{description}.sql` (applied in ascending `{n}`); one
  query per file lives in `src/main/sql/dml/{name}.sql`. The `dml` file stem
  (kebab-case) becomes the `snake_case` function name; a generated row struct is
  `<Pascal>Row`. The return shape is inferred from the statement — a `SELECT`
  returns `[Row]`, anything else returns the rows-affected `number`.
- **Schema is `CREATE TABLE` only.** Other statements in a migration (indexes,
  views, `PRAGMA`, …) are ignored when building the type catalog, but still run
  through `migrate`.
- **One statement per query file.** A `dml` file with multiple statements is
  rejected. A migration file may hold several `;`-separated statements.

## Type-mapping assumptions

- **Keyword matching, not exhaustive `DataType` enumeration.** A column's type is
  classified from the lower-cased type-name string: `bool*`→`bool`;
  `real/double/float/decimal/numeric`→`number` (`Real`);
  `*int*`/`serial`→`number` (`Int`); everything else (`text/varchar/char/clob/
  uuid/json/date/time/blob/binary/…`)→`string`. Resilient across dialects, but
  imprecise — e.g. `DATE`, `JSON`, and `BLOB` all collapse to `string`.
- **`number` is one neoc type.** `Int` vs `Real` only affects *binding* and the
  decoder accepts either for a `number` column (`SqlValue::Int | SqlValue::Real`).
- **Nullability folds schema + joins + expressions.** A column is `Option<T>`
  unless `NOT NULL`/`PRIMARY KEY`; an outer join widens it (`LEFT` nulls the
  joined table, `RIGHT` the existing ones, `FULL` both); an aggregate or
  expression carries its own rule (`COUNT` non-null, `SUM`/`AVG`/`MIN`/`MAX`
  nullable, `COALESCE` non-null if any argument is, a `CASE` without `ELSE`
  nullable).
- **No `Bytes`/temporal/JSON `SqlValue` variants.** `SqlValue` is
  `Int | Real | Text | Bool | Null`; binary/temporal/JSON ride along as `Text`.

## Query-inference scope

Supported:

- **`SELECT`**: multiple tables and `INNER`/`LEFT`/`RIGHT`/`FULL`/`CROSS` joins;
  alias-aware column resolution (qualified `t.col` and unqualified, with an
  ambiguity error); `*` and `alias.*` wildcards; aliased and unaliased
  expressions — arithmetic, `||` concat, comparisons, `CASE`, `CAST`, `COALESCE`,
  and a function catalog (aggregates, `UPPER`/`LOWER`/`SUBSTR`/…, `LENGTH`/`ABS`/…).
- **Parameters** from `WHERE` and `HAVING`: `col <op> ?`, `BETWEEN ? AND ?`,
  `LIKE ?`, `IN (?, ?, …)` (fixed arity), and `LIMIT`/`OFFSET ?`. Each typed from
  the column or expression opposite the placeholder.
- **`INSERT … (cols) VALUES (…)`**: params from the column list; optional
  `RETURNING` makes it row-returning.
- **`UPDATE … SET … WHERE …`**: params from `SET` values (incl. `col = col + ?`)
  then `WHERE`; optional `RETURNING`.
- **`DELETE … WHERE …`**: params from `WHERE`; optional `RETURNING`.

- **CTEs and derived tables**: `WITH name [(cols)] AS (…)` and
  `FROM (SELECT …) AS name [(cols)]` expose the defining query's result columns
  — type and nullability — to the rest of the statement. Later CTEs see earlier
  ones; a CTE shadows a schema table of the same name. An outer join to either
  widens its columns to optional, exactly as for a table.
- **Subqueries in expressions**, anywhere a `SELECT` list item, `WHERE`,
  `HAVING`, or `JOIN … ON` allows an expression (including `UPDATE`/`DELETE`
  `WHERE`). A subquery sees its own `FROM` first, then enclosing queries
  (correlated references), nearest first. Typing follows SQL: a scalar
  subquery `(SELECT …)` has its column's type and is always optional (no row is
  NULL); `EXISTS` is a non-null `bool`; `x IN (SELECT …)` is a `bool`, optional
  when `x` or the subquery column is. A scalar or `IN` subquery must project
  exactly one column. Its result column names are never read, so an unaliased
  expression needs no alias there.
- **Set operations** (`UNION`/`INTERSECT`/`EXCEPT`, with or without `ALL`, nested
  and parenthesised). Both sides must project the same number of columns and the
  same neoc type position by position. Field names come from the left side. A
  column is optional for `UNION` if either side is, for `EXCEPT` if the left is,
  for `INTERSECT` only if both are.
- **Window functions** (`f(…) OVER (…)`). `PARTITION BY`/`ORDER BY` columns are
  resolved. `row_number`/`rank`/`dense_rank`/`ntile`/`cume_dist`/`percent_rank`
  are non-null `number`; `lag`/`lead` take their argument's type and are
  optional unless a non-null default (third argument) is given and the value is
  non-null; `first_value`/`last_value` follow their argument; `nth_value` is
  optional; an aggregate over a window types as the aggregate does
  (`count` non-null, `sum`/`avg`/`min`/`max` optional). `OVER w` (a named window)
  is accepted as written.

Rejected with a clear `Error::Unsupported`/`Error::Resolve` diagnostic:
`WITH RECURSIVE`, `LATERAL` and unaliased derived tables, other `FROM` forms
(table functions, nested joins), `VALUES`/`TABLE` as a query body, set-operation
sides of differing arity or type, an unaliased non-column/non-function
projection (outside an `EXISTS`/`IN`/scalar subquery), a bare `NULL` in the
SELECT list, an unknown table/column, and an ambiguous unqualified column.
`INSERT … SELECT` takes a plain `SELECT` only (no `WITH` or set operation).

Known inference limitations (documented, not yet closed):

- **Parameter order** is collected from `SET` → `WHERE`/`HAVING` → `LIMIT`/
  `OFFSET`. Placeholders in other positions (the projection outside a subquery,
  `ORDER BY`, a window specification) are not collected, so generation fails with
  a placeholder-count mismatch rather than misbind them. Placeholders in a CTE,
  derived table, or subquery bind in textual order.
- **`INSERT` placeholder count** is not validated against the `VALUES` list.
- **`IN (?)`** binds one parameter per placeholder; there is no variadic
  list-parameter form yet.
- **Type mapping** is by keyword (above), so `DATE`/`JSON`/`BLOB` are `string`.

- **Parameter naming** is the bound column's name; a name used more than once
  gets `_2`, `_3`, … suffixes (same for result fields, e.g. a `*`-join collision).
- **Placeholders are emitted verbatim.** sqlc-style queries carry the dialect's
  own placeholders (`?`, `$1`), so autosql does not rewrite them — the dialect
  affects *parsing* and (in future) synthesised SQL, not the emitted query text.

## neoc validity constraints the emitter respects

What counts as *well-formed neoc* (per `lang/`, confirmed by type-checking the
output); the generated code is shaped to stay inside it:

- **A `match` arm may contain neither `return` nor `?`** (the language rejects a
  control-flow expression in a match arm's value position). Decoding therefore
  uses shared `Result`-returning helpers (`decode_number`, `decode_opt_string`,
  …) whose `match` yields `Ok`/`Err` values, consumed with `?` at statement
  level.
- **No list patterns, no `len`/`push`.** `:many` materialises with
  `let out: [Row] = []; … out[next] = …; next = next + 1;` inside
  `for row in rows` (index-assignment appends). `:one` uses
  `for … { … break; }`. (neoc has no `mut`: `let` bindings are reassignable —
  `const` is the immutable form.)

## Output-shape assumptions

- **Contract is inlined into the one DAO module** (see "Output is one
  self-contained file" above) — no separate `sql.neoc`.
- **Naive singularisation** for model names: strip one trailing `s` (`users` →
  `User`; `bus` → `Bu`). Irregular plurals not handled.
- **Result reuses the model struct** when a projection is exactly the table's
  columns in order; otherwise a `<Name>Row` struct is emitted.
- **SQL is whitespace-collapsed** to a single line for the embedded string
  literal; `\` and `"` are escaped.
- **No formatter pass.** Output is emitted pre-formatted by hand. Running it
  through `neoc fmt` is a possible future step (it would also validate parses).

## Out of scope (by design)

- Running the generated code against a real database — that needs a user
  `impl SqlExecutor` over `ext` bindings to a driver (`vnd:rusqlite`,
  `vnd:sqlx_*`, a JS driver, …). autosql produces the neoc; it does not execute
  it. Tests prove the output is well-formed neoc, not that it runs.
- Conformance pairs under `lang/CONFORMANCE` — autosql is tooling, not a
  language construct; the conformance suite covers the language itself.
