---
name: clean-code-review
description: Clean-code audit of a change (the current diff or a branch) in this repo, neoc or vos, against SRP, DRY, naming, coupling, dead code, KISS, BOUNDARY (neoc-vm takes no vos concept; the vos crate ports, per-app VM isolation (not a security boundary), the protocol and the atomic layers) and PANIC (unwrap and panics in Rust, hostile values into a module, unvalidated JSON-RPC params, unchecked casts, non-null assertions, agent or tool input). Fixes what this session introduced and tags the rest as scored TODO markers. Use after implementing a change and before the pre-commit skill; pr-review delegates its clean-code pass here.
---

# Clean-code review

## Scope

Review the changed files (`git diff origin/main...HEAD`, plus uncommitted work) and the code they touch directly. Skip test
boilerplate, framework-required shapes (Storybook CSF, Playwright fixtures, a `UserData` impl a trait demands), generated
files (`types/neoc.d.luau`, `types/generated/*`) and pre-existing issues outside the diff. Lint already covers formatting,
clippy, the crate-direction check, atomic layers, test ids and Luau types; don't report what `bun run lint` or
`scripts/check-neoc.sh` reports.

**Proportionality.** Iteration speed is a requirement (CLAUDE.md, "Non-functional requirements"): the audit scales with the
change. A small or ordinary diff (up to about 400 changed lines and 10 files) gets one inline pass over the principles, no
sub-agents. Run sub-agents, one per principle group (1 to 3, 4 to 6, 7, 8), each given the diff and the file list, not your
conclusions, only for a large or risky diff: more than about 400 changed lines or 10 files, or a change to the VM's embedding
API or sandbox, the protocol, the host's HTTP surface or a crate boundary. Each agent reports file, line range, principle, a
one-line description and a severity from 0 to 1.

## Principles (both products)

1. **SRP**: a function, use case or component doing two jobs; mixed I/O and logic; a type with several reasons to change.
   vos: an adapter holding policy that belongs in a use case or the domain; a Luau `render` or handler holding logic that
   belongs in a plain function (testable from Rust); a Svelte page mixing RPC, parsing and layout instead of using the
   renderer core.
2. **DRY**: copy-pasted blocks, duplicated constants, near-identical functions, repeated conditionals. vos: a renderer
   adapter or an app re-implementing a control the kit has or should have; a wire shape written by hand that the Rust schema
   generates. neoc: a helper the module facade already offers re-written inside a module (the `ok` / `from_err` helpers,
   the `Module` builder).
3. **NAMING**: names that hide intent (`data`, `handle`, `manager`, `util`, `processor`). neoc: error prefixes are
   `module.function:`, a `UserData` type is named after its Rust type, constructors follow the table in `CLAUDE.md`.
   vos: tool, test-id and node-key names break the conventions (`<appId>__<tool>`, `[a-z][a-z0-9_]*` local tool names, test
   ids from keys); error variants describe the mechanism instead of the failure.
4. **COUPLING**: concrete dependencies built inline instead of injected (vos: a `Clock`, `Fetcher` or `Notifier` created
   inside a use case); modules reaching into each other's internals; shared mutable state (Rust `static`s, Luau globals, a
   Svelte module outside a `*.svelte.ts` store); a use case that knows an adapter's types.
5. **DEAD**: unused functions, ports, enum variants, props, handlers, tools or stories; unreachable branches; commented-out
   code; stale imports.
6. **KISS**: abstraction without a second user (neoc: a helper with fewer than 3 call sites), premature options or traits,
   `$effect` where `$derived` does, a generic where a concrete type will do. Ask why up to five times; if no answer
   survives, it is a violation.
7. **BOUNDARY**: coupling across a real seam that lint can't see. It needs a named thing that changes independently (an
   adapter's library, an app, an agent, the client, the protocol version, the embedder of neoc-vm); if you can't name one,
   it is not a finding (KISS).
   - **neoc-vm and vos**: a vos concept in `neoc-vm` or `neoc` (apps, `readOnly`, views, `app.*`, a vos module, a type or
     name that only makes sense to vos) belongs in `vos-lua` instead. neoc-vm gets only a generic mechanism an embedder
     needs (a hook, a registry). Anything a module starts that outlives the call (a task, server, listener, worker) is
     registered with the engine's `Life` so shutting the script down stops it; an async host function is made with
     `lifecycle::async_function` and the right `Role`, never a raw `create_async_function`.
   - **crate ports** (vos): a port that leaks adapter types (`rusqlite`, `mlua`, `axum`, `rmcp` in a signature of
     `vos-application` or `vos-domain`); a driving adapter reaching into a driven one instead of a use case; a use case that
     bypasses a port; policy (grants, `readOnly`) enforced in an adapter instead of the layer that owns it.
   - **app state hygiene** (the VM is not a security boundary): a new `app.*` capability that exposes another app's data or
     state; a module or library that opens a listener, task, worker or pool without registering it with the app's `Life`;
     anything that lets a VM outlive its unload.
   - **the protocol**: a JSON-RPC method, view message or AG-UI event that changes without the Rust schema,
     `bun run codegen` output and the client's parse step moving together; a side channel (extra route, raw socket) around
     `/rpc`, `/agui` or `/mcp`.
   - **the client**: app knowledge in the client or the kit (the client runs no app code), executable data crossing the
     wire, an adapter that writes to the host directly instead of through a Binding or event; atomic layers in the kit and
     client.
   - **agents and apps**: an app reaching another app other than through tools; an agent reaching a tool its grants don't
     cover; per-agent identity not carried into a call.
   Flag also a resource a use case or adapter opens that its unload path doesn't release.
8. **PANIC**: a reachable crash or silent corruption from outside input. It needs a named triggering input: a script value
   (a Lua number that sizes an allocation, a cyclic or huge table, a poisoned `__tostring` error value), a client message, an
   agent or tool payload, an app's return value, a value stored by an older version, a file, the network. Tests are exempt. A
   cast, `unwrap` or `!` guarded by an invariant established just above is fine; ask for a comment stating the invariant.
   - **Rust:** `unwrap`, `expect`, `panic!`, `unreachable!` (clippy denies the first three in the vos crates; the neoc crates
     do not inherit the lint table, so check them by hand), indexing or slicing that can fail, `as` casts that truncate,
     arithmetic that can overflow, a lock held across `.await`; on input from a script, JSON-RPC params, tool arguments, app
     output, ACP messages, app-ref contents, SQL rows from an older version, files, the network. A host panic aborts every
     app under `release-host` (`panic = "abort"`).
   - **neoc modules:** a function that takes a script value and trusts its type, range or shape (the wrong type raises; a
     recoverable failure returns `(nil, "module.function: message")`); a size, sleep or index that is not bounded; a
     module not covered by `crates/neoc-vm/tests/hostile_inputs.rs` (it sweeps `modules::names()`).
   - **JSON-RPC params** (vos): a method that reads `params` without parsing it into a typed message (by-name params,
     `-32602` on a bad shape).
   - **Luau apps:** a value from `app.kv`, a db row, a tool input or `e.payload` used without checking its shape (a type
     annotation or a `::` cast is not a check); SQL built by string concatenation.
   - **TypeScript and Svelte:** `as T` over `unknown` (RPC and AG-UI payloads, `JSON.parse`, stored client state); a non-null
     `!`; indexing assumed defined; a throw inside a subscription or `$effect` that nothing catches; a fallible promise fired
     with `void` and no catch; `{@html}` or `innerHTML` with agent or tool text.

## Report and tag

Above 0.5: fix the ones this session introduced now; tag the rest where they occur, one marker per violation:

```rust
// TODO: clean-code - 0.7 - PANIC: `rows[0]` on a query result an older app version may have left empty; return PortError::Rejected
```

```lua
-- TODO: clean-code - 0.6 - PANIC: `app.kv:get("history")` trusts data an older version wrote; check its shape
```

In TypeScript use `// TODO: clean-code - <score> - <PRINCIPLE>: ...`, and in Svelte markup
`<!-- TODO: clean-code - <score> - <PRINCIPLE>: ... -->`. `<PRINCIPLE>` is SRP, DRY, NAMING, COUPLING, DEAD, KISS, BOUNDARY or
PANIC. List markers with
`git grep -nE "TODO: clean-code - [0-9]" -- crates apps packages client scripts e2e types tooling`. The `refactor` skill
clears markers one at a time; `pre-commit` blocks on any above 0.5.
