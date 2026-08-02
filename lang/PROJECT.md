# Projects

Defines the **project** — how a tree of `.neoc` source files is named, laid out on disk, discovered, and built. A project is the unit a build tool operates on. Single files compile without one; the project model is opt-in and adds nothing a lone file needs.

This file is host-agnostic. It describes the manifest, the layout, the root-discovery rule, and where output lands. It says nothing about how a particular tool is implemented.

---

## 1. The manifest

A project is rooted at a directory containing a **manifest** named `neoc.toml`. The manifest declares the project's identity and its default build target:

```toml
[package]
name = "my-app"
target = "luau"
```

- `[package]` — required table.
  - `name` — required string. The project's name.
  - `target` — required string, `"luau"` (the only target). The default build target (see `DYNAMIC.md` for what it emits). A build may override it.

Unknown keys are ignored, so the schema may grow without breaking older manifests. A manifest that is not valid TOML, or that omits `[package]`, `name`, or a recognised `target`, is a project error reported in neoc terms.

---

## 2. Layout

A project uses a fixed directory layout under its root (the directory holding `neoc.toml`):

```
<root>/
  neoc.toml
  src/main/neoc/      neoc source
  src/main/sql/       SQL inputs for SQL-derived modules
  src/auto/<target>/  build output (src/auto/luau/) — generated
```

- `src/main/neoc/` is the **source root**. All module resolution anchors here (§4).
- `src/main/sql/` holds SQL inputs that generate neoc modules into `src/main/neoc/`.
- `src/auto/<target>/` holds the build output: the generated `src/auto/` tree mirrors the hand-written `src/main/` tree under a subdirectory named for the target (`src/auto/luau/` when `target = "luau"`), each module keeping its path under the source root (`src/main/neoc/a/b.neoc` → `src/auto/<target>/a/b.<ext>`). The output is generated, not source; whether to track it is the project's choice (an embeddable example may commit it; an application typically ignores it). An explicit output directory overrides this default.

The entry module of a project is `src/main/neoc/main.neoc`.

---

## 3. Root discovery

A tool given a file (or working directory) finds the project root by walking **up** the directory tree from that file's directory, testing each ancestor for a `neoc.toml`, and stopping at the first one found. That directory is the project root.

If no ancestor contains a `neoc.toml`, the file is **not in a project**. Resolution then falls back to the single-file rule (§4): the root is the directory containing the file itself, and modules resolve as siblings of it. A lone `.neoc` file, or a directory of them with no manifest, therefore compiles exactly as it did before projects existed — the manifest changes nothing by its absence.

The same discovery rule serves building, checking, and editor diagnostics, so a tool and an editor never disagree about where a project begins or where a module lives.

---

## 4. Module resolution

A `use a::b::Item;` names the module `a::b` (every segment but the last) and the item `Item`. The module resolves to a single source file:

- **In a project:** relative to the source root. `use a::b::Item` → `<root>/src/main/neoc/a/b.neoc`; a single-segment-qualifier `use foo::Item` → `<root>/src/main/neoc/foo.neoc`.
- **Outside a project (no manifest found):** relative to the directory of the importing file. `use a::b::Item` → `<dir>/a/b.neoc`; `use foo::Item` → `<dir>/foo.neoc`.

In both cases the rule is identical once the anchor directory is fixed — projects only change *which directory* is the anchor (the source root vs. the importing file's directory). A `use` whose module path resolves to no file under the anchor is a resolution error (`STATIC.md` §2).

---

## 5. Building

Building a project with no explicit source path is manifest-driven:

1. Discover the root (§3).
2. Read `target` from the manifest (an explicit target override wins).
3. Compile from the entry module `src/main/neoc/main.neoc`, following its transitive `use` graph (§4).
4. Write each emitted module to `src/auto/<target>/`, preserving the module's path under the source root (`src/main/neoc/a/b.neoc` → `src/auto/<target>/a/b.<ext>`). An explicit output directory overrides this.

A build given an explicit file path instead compiles that file as the entry, using single-file resolution when the file is not in a project (§3–§4). This is the form a lone file or a glob uses.

Checking performs steps 1–3 and reports diagnostics without writing output.

---

## 6. SQL- and foreign-derived modules

Tools that generate `.neoc` modules from other inputs follow the layout by default when run inside a project: SQL-derived modules read from `src/main/sql/` and write to `src/main/neoc/`; modules derived from foreign type declarations write to `src/main/neoc/`. Explicit input/output paths override the project defaults, so the same tools work on a lone file with no manifest.
