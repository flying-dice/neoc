# Modules + visibility

- **Status:** done
- **Commits:** `6178a3b` (types visibility), `96083f8` (compiler driver),
  `5e79c29` (CLI multi-file build)

## Outcome

`transpile_project(entry, target)` BFS-resolves the transitive `use` graph
(cycle-safe): `use a::b::c` → module `a::b` at `<root>/a/b.neoc`, item `c`,
`<root>` = the entry's dir. `collect` records each `pub` item's exported names;
the driver injects a module's allowed `pub` symbols into each importer and checks
bodies via `check_with_table`. A `use` of a non-`pub` item →
``` `c` is private in module `a::b` ```; an unresolvable path → `unresolved
module \`a::b\``. `use`→import: Lua `local c = require("a.b").c`, JS
`const c = require("a/b").c`, Python `from a.b import c as c`. `neocc build
<entry>` emits the whole graph. Single-file (no `use`) is byte-identical — the
30-case corpus stays green (212). 439 tests total.

Deferred: nested `mod`, glob `use a::b::*`, `pub use` re-exports.

---

(original spec follows)

- **Area:** `crates/neocc-types` (resolution), `crates/neocc-compiler` (driver)
- **Priority:** P1
- **Depends on:** `cli.md` is the natural driver for multi-file builds

## Goal

Finish `pub`/visibility, which is half-wired and not enforced because the checker
is **single-module**. Today `pub` only changes Lua emission (global vs `local`);
nothing checks that a `use`d item is actually `pub`, and there is no multi-file
compilation unit.

Two layers:

1. **Multi-file module resolution.** Resolve a `use a::b::c;` path to another
   source file/module, load + check it, and make its `pub` items available.
   Decide the module→file mapping (path-based, mirroring the `require("a.b").c`
   lowering already in `DYNAMIC.md` §3.7).
2. **Visibility enforcement** (`STATIC.md` §2): using a non-`pub` item from
   another module is an error; within a module everything is visible.

## Acceptance

- [ ] A program spanning two files type-checks, resolving a `pub` item across
      them; a `use` of a private item is a diagnostic.
- [ ] Emission still maps `pub`→global, private→`local`, and `use`→`require`.
- [ ] Tests covering cross-module resolution + a visibility violation.

## Notes / pointers

- This is the concrete meaning of "finish visibility" — it needs the module
  system, so the two are one task.
- `neocc-types::check` is currently single-`Module`. Multi-module likely means a
  driver that collects all modules' `SymbolTable`s first, then checks bodies.
