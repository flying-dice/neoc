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

Rejected with a clear `Error::Unsupported`/`Error::Resolve` diagnostic:
subqueries and CTEs, set operations (`UNION`/…), window functions, derived
tables, an unaliased non-column/non-function projection, a bare `NULL` in the
SELECT list, and an unknown table/column or ambiguous unqualified column.

Known inference limitations (documented, not yet closed):

- **Parameter order** is collected from `SET` → `WHERE`/`HAVING` → `LIMIT`/
  `OFFSET`. Placeholders in other positions (a `JOIN … ON`, the projection, a
  subquery) are not collected, so a positional driver would misbind them.
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
