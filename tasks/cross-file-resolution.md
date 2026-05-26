# Cross-file resolution for `neoc-lsp`

- **Status:** in-progress (design)
- **Priority:** P2
- **Depends on:** `ide-feature-parity` (the single-file handlers this generalises)

The single biggest tooling gap: every feature is file-local. References, rename,
workspace symbols, completion-resolve, and the scope-sensitive lints all stop at
the file boundary. This is the enabler that makes them project-wide — and the
hard constraint is **performance**: snappy at a few dozen files, graceful at
thousands. It must never re-analyse the world on a keystroke.

This document is the architecture and the phased plan, grounded in how the major
servers do it. Implementation lands in phases, each its own MR.

## What the major servers teach (and what we take)

Researched rust-analyzer (salsa), TypeScript tsserver, gopls, clangd. The
consensus for staying fast:

- **Laziness beats brute incrementality** (rust-analyzer): compute from the
  cursor outward; never eagerly analyse off-screen code.
- **Keep the hot unit small** (gopls): memory/work `O(open + touched)`, not
  `O(workspace)`. gopls's whole-program-in-memory model cost ~30× source size
  and took a year to undo — don't start there.
- **Index, don't scan, for project-wide queries** (gopls, clangd): a
  `name → locations` index answers references/workspace-symbols without
  re-analysing files. Naive "scan every file per query" is the `O(files²)` trap.
- **Text-first, semantics-second for find-usages** (rust-analyzer): filter to
  files that even contain the identifier text, scope by visibility, resolve only
  the survivors — `O(candidates)`, not `O(files)`.
- **Targeted invalidation with early cutoff**: an edit re-parses one file; it
  invalidates dependents only if the file's *exported surface* changed (a
  body/whitespace edit must not cascade). rust-analyzer's `ItemTree` is the
  body↔structure firewall; gopls prunes on export-summary diff.
- **Per-file cache keyed by version/hash**; open buffers authoritative, disk
  fallback. Stale-cache-vs-dirty-buffer is the top correctness trap.
- **Debounce + cancellation**: debounce workspace work (~300–500ms idle); a new
  edit cancels in-flight reads (revision bump → bail). Heavy work off the
  request path.

Sources: rust-analyzer architecture + "Three Architectures for a Responsive
IDE" + "Durable Incrementality" + Find Usages blog; gopls "Scaling gopls" blog +
issue 57987; clangd indexing design; LSP 3.17 spec; VS Code LSP guide.

## neoc-specific design

neoc's advantage: **resolution is already name-based**, and the compiler already
resolves a project. We do not need salsa or a type-level module model to start.

**Reuse the compiler's module resolver — do not reinvent it.** `neoc-compiler`
already follows an entry's `use` declarations to sibling `.neoc` modules
(`transpile_project`, behind `neoc build`/`check`). The LSP's file graph **must
match the compiler's**, or goto/rename will disagree with what compiles. Lift
the resolution (entry → module graph: the set of reachable files and the `use`
edges between them) into a reusable `neoc-compiler` API the LSP consumes. The
golden rule applies: one resolution rule, shared.

### The MVP (Phase 1–3): in-memory, name-based, targeted

```
WorkspaceIndex
  documents:   uri → { version, text }      open buffers authoritative; disk fallback by hash
  parsed:      uri → (CST, declarations)    cached, keyed by (uri, version|hash)
  decls:       name → [(uri, kind, range)]  every declaration, project-wide
  imports:     uri → [module paths]         from `use`; resolved to uris via the compiler rule
  importers:   uri → [uri]                  reverse-dependency map (bounds invalidation)
```

- **Discovery** — workspace folders + glob `**/*.neoc`, skipping build/vendor
  dirs; lazy/background, not a blocking startup scan. Client file-watchers
  (`workspace/didChangeWatchedFiles`, narrow glob) drive on-disk invalidation.
- **Per-file work** — parse + extract declarations/`use` once per `(uri,
  version)`; an edit re-does only that file.
- **Targeted invalidation** — on change, reparse the edited file; recompute its
  `decls`/`imports` entries; **early cutoff**: if its exported declaration set
  and import set are unchanged, no dependent is touched. Otherwise invalidate via
  `importers`.
- **Queries read the index**:
  - references/rename → text-prefilter candidate files (the name must appear),
    then resolve against the index; scope by visibility (private → its file).
  - workspace symbols → fuzzy over `decls` (generalises today's open-doc scan).
  - completion auto-import → `decls` of `pub` items not yet imported, with the
    `use` edit.
- **Cancellation** — a per-revision token; long reads check it and bail. Adopt
  this cheaply now even single-threaded.

### Complexity targets

- Edit: `O(size of edited file)` + (only if its surface changed) reindex of
  direct importers. Never `O(workspace)`.
- references/rename: `O(files containing the name)` resolution, not `O(files)`.
- workspace symbols: index lookup, not a scan.
- Memory: `O(files)` for the name index + open buffers; **no** retained ASTs or
  type models across edits — store `(uri, range)` and re-resolve.

## Pitfalls to avoid (from the research)

- Re-parsing/re-indexing the project on one edit — the classic killer. Per-file.
- `O(files²)` reference search — index + text-prefilter + visibility scope.
- Holding every file's AST/typed tree in memory — keep coarse summaries; drop
  bodies; re-resolve from `(uri, range)`.
- No early cutoff — a body edit cascading to dependents defeats incrementality.
- Cache keyed on path only — key on version/hash or risk silent staleness.
- Serving stale disk content for an open dirty buffer — open buffers win.
- Eager recursive scan incl. build/vendor dirs — lazy discovery, narrow globs.
- No debounce/cancellation — keystroke storms; superseded work must abort.
- Persisting the semantic model — recompute; don't lock into a serialised format.

## Deferred — with the trigger that earns each

- **Salsa-style general query engine / durability levels** — only if hand-rolled
  version-keyed caching + the reverse-dep map sprawls. Single revision counter is
  fine at hundreds of first-party files.
- **Sharded FST symbol index / trigram index** — when workspace-symbol or
  find-usages latency on a large project is *measured* to hurt. A sorted vec +
  text-prefilter suffices first.
- **On-disk persisted index** — when cold-start rebuild time hurts. In-memory
  first.
- **Background thread pool + snapshots** — when synchronous index builds get
  slow. Adopt revision-checked early-exit now; full threadpool later.
- **Two-tier export-summary type model** — premature until memory is the
  bottleneck; the name index is syntactic.

## Open questions

- **Module → file rule.** Pin exactly how the compiler maps a `use` path to a
  file (sibling `name.neoc`? nested dirs?) and expose it as the shared API. The
  LSP must not diverge. (Read `transpile_project` + the resolver it calls.)
- **Workspace root.** Without a manifest, what is the project root for discovery
  — the LSP `rootUri`/workspace folders? Confirm the client sends them.
- **`std::`/`lib::`/`vnd::` imports** are runtime/`ext`, not project files —
  excluded from the file graph (handled by the `ext` seam).

## Phased plan

1. `[x]` **Module-graph API in `neoc-compiler`** — `module_file` is the shared
   `use`-path → file rule, exposed and unit-tested (commit `a06bad3`,
   `tests/module_file.rs`). The `WorkspaceIndex` resolves imports through it.
2. `[x]` **`WorkspaceIndex` foundation in `neoc-lsp`** — `crates/neoc-lsp/src/workspace.rs`:
   content-hash-keyed per-file summary (declarations + `use` imports), the
   `decls` name index, the reverse-dependency `importers` map, and `set_document`
   returning a `SurfaceChange` early-cutoff signal. Imports resolve via
   `module_file`. Built and unit-tested in isolation; `#[allow(dead_code)]` until
   Phase 3 wires it. (12 unit tests.)
3. `[x]` **Wire cross-file queries** — landed across four slices on
   `feat/lsp-ide-feature-parity`.
   - **Lifecycle** `[x]` — `Backend` holds an `Arc<Mutex<WorkspaceIndex>>`, rooted
     from `initialize` (`workspace_folders`/`root_uri`), maintained in
     `did_open`/`did_change`/`did_close`. Dead-code allow dropped.
   - **Discovery** `[x]` — `initialized` spawns a `spawn_blocking` walk of the root
     for `.neoc` files (skips `target`/`node_modules`/`.git`, open buffers win); no
     synchronous startup scan. `workspace/didChangeWatchedFiles` on-disk
     invalidation is still TODO (open/edit re-indexes through the lifecycle).
   - **Workspace symbols** `[x]` — the handler reads `index.workspace_symbols`,
     project-wide over every indexed file.
   - **References / rename** `[x]` — `reference_scope` bounds the file set by
     visibility (a `pub` decl's defining file + its importers; private/local stays
     in-file) and `references_by_name`/`rename_by_name` rewrite the other files.
     Within a scoped file the match is still text-level — the Phase 5 scope work
     stops the remaining over-approximation.
   - **Goto-definition** `[x]` — follows `use` across files, scoped to the file's
     own import targets so it never lands in an unrelated module.
   - **Tests** — every new unit covered; the multi-file cucumber harness (`Given
     the file "<path>":`) is in, exercising workspace symbols across files.
     Extending the multi-file specs to goto/references/rename is follow-up polish.
4. `[ ]` **Auto-import completion** — `pub` decls from other files, with the
   `use` insertion edit.
5. `[ ]` **Scope-aware resolution** — real name resolution (scopes, shadowing,
   imports) so references/rename stop over-approximating and the scope-sensitive
   lints (unused `let`/param, shadowing) land.
6. `[ ]` **Debounce + cancellation** — revision token, debounced workspace
   diagnostics. (Adopt the token in Phase 2; tune here.)

Benchmark corpus: `samples/openfeature/` (multi-file neoc) is the realistic
target to measure index build + query latency against.
