---
name: vos-clean-code-review
description: Clean-code audit of a vos change (the current diff or a branch) against SRP, DRY, naming, coupling, dead code, KISS, BOUNDARY (the crate ports, per-app VM isolation (not a security boundary), the protocol and the atomic layers) and PANIC (unwrap and panics in Rust, unvalidated JSON-RPC params, unchecked casts, non-null assertions, agent or tool input). Fixes what this session introduced and tags the rest as scored TODO markers. Use after implementing a change and before the vos-pre-commit skill; vos-code-review delegates its clean-code pass here.
---

# Clean-code review

## Scope

Review the changed files (`git diff main...HEAD`, plus uncommitted work) and the code they touch directly. Skip
test boilerplate, framework-required shapes (Storybook CSF, Playwright fixtures, generated files under `types/generated`
and `docs/reference`) and pre-existing issues outside the diff. Lint already covers formatting, clippy, the crate-direction
check, atomic layers, test ids and Lua types; don't report what `bun run lint` reports.

More than 50 changed lines or more than 3 files: run one sub-agent per principle group (1 to 3, 4 to 6, 7, 8), each
given the diff and the file list, not your conclusions. Otherwise one inline pass.

## Principles

1. **SRP**: a function, use case or component doing two jobs; an adapter holding policy that belongs in a use case or the
   domain; a Lua `render` or handler holding logic that belongs in a plain function (testable from Rust); a Svelte page
   mixing RPC, parsing and layout instead of using the renderer core.
2. **DRY**: copy-pasted blocks, constants or near-identical components, especially a renderer adapter or an app
   re-implementing a control the kit has or should have; a wire shape written by hand that the Rust schema generates.
3. **NAMING**: names that hide intent (`data`, `handle`, `manager`, `util`); tool, test-id and node-key names that break
   the conventions (`<appId>__<tool>`, `[a-z][a-z0-9_]*` local tool names, `docs/TESTING.md` test ids from keys); error
   variants that describe the mechanism instead of the failure.
4. **COUPLING**: concrete dependencies built inline instead of injected through a port (a `Clock`, `Fetcher`, `Notifier`
   created inside a use case); global or module-level mutable state (Rust `static`s, Lua globals, a Svelte module outside
   a `*.svelte.ts` store); a use case that knows an adapter's types.
5. **DEAD**: unused functions, ports, enum variants, props, handlers, tools or stories; unreachable branches;
   commented-out code.
6. **KISS**: abstraction without a second user, premature options or traits, `$effect` where `$derived` does, a generic
   where a concrete type will do. Ask why up to five times; if no answer survives, it is a violation.
7. **BOUNDARY**: coupling across a real vos seam that lint can't see. Seams:
   - **crate ports**: a port that leaks adapter types (`rusqlite`, `mlua`, `axum`, `rmcp` types in a signature of
     `vos-application` or `vos-domain`); a driving adapter reaching into a driven one instead of a use case; a use
     case that bypasses a port; policy (permissions, grants, `readOnly`) enforced in an adapter instead of the layer that owns it;
   - **app state hygiene** (the VM is not a security boundary): a new `app.*` capability that exposes another app's data or state, a module or library that
     opens a listener, task, worker or pool without registering it with the app's `Life`, anything that lets a VM
     outlive its unload;
   - **the protocol**: a JSON-RPC method, view message or AG-UI event that changes without the Rust schema,
     `bun run codegen` output and the client's parse step moving together; a side channel (extra route, raw socket) around
     `/rpc`, `/agui` or `/mcp`;
   - **the client**: app knowledge in the client or the kit (the client runs no app code), executable data crossing the
     wire, a client adapter that writes to the host directly instead of through a Binding or event;
   - **agents and apps**: an app reaching another app other than through tools; an agent reaching a tool its grants
     don't cover; per-agent identity not carried into a call;
   - **atomic layers** in the kit and client.
   Flag also: a resource a use case or adapter opens that its unload path doesn't release.
8. **PANIC**: a reachable crash or silent corruption from outside input.
   - **Rust:** `unwrap`, `expect`, `panic!`, `unreachable!`, indexing or slicing that can fail, `as` numeric casts that
     truncate, arithmetic that can overflow, a lock held across `.await`; on input from JSON-RPC params, tool arguments, app
     (Lua) output, ACP messages from an agent process, app-ref contents, SQL rows from an older version, files, the network.
   - **JSON-RPC params:** a method that reads `params` without parsing it into a typed message (by-name params, `-32602` on a
     bad shape).
   - **Lua:** a value from `app.kv`, a db row, a tool input or `e.payload` used without checking its shape (LuaLS
     `---@type` annotations are not a check); SQL built by string concatenation.
   - **TypeScript and Svelte:** `as T` over `unknown` (RPC and AG-UI payloads, `JSON.parse`, stored client state); a non-null
     `!`; indexing assumed defined; a throw inside a subscription or `$effect` that nothing catches; a fallible promise fired
     with `void` and no catch; `{@html}` or `innerHTML` with agent or tool text.

## Gates on BOUNDARY and PANIC

- **BOUNDARY** needs a real seam: name the thing that changes independently (an adapter's library, the app, the agent,
  the client, the protocol version). If you can't, it is not a finding (KISS).
- **PANIC** needs a named triggering input from outside: a client message, an agent or tool payload, an app's return
  value, a stored value from an older version, a file, the network. Tests are exempt. A cast, `unwrap` or `!` guarded by an
  invariant established just above is fine; ask for a comment stating the invariant. (`unwrap` and `expect` are denied by
  clippy outside tests anyway, so in Rust this principle is mostly casts, indexing, arithmetic and unvalidated input.)

## Report and tag

Report each finding with file, line range, principle, a one-line description and a severity from 0 to 1. Above
0.5: fix the ones this session introduced now; tag the rest where they occur:

```rust
// TODO: clean-code - 0.7 - PANIC: `rows[0]` on a query result an older app version may have left empty; return PortError::Rejected
```

```lua
-- TODO: clean-code - 0.6 - PANIC: `app.kv:get("history")` trusts data an older version wrote; check its shape
```

In TypeScript use `// TODO: clean-code - <score> - <PRINCIPLE>: ...`, and in Svelte markup
`<!-- TODO: clean-code - <score> - <PRINCIPLE>: ... -->`. The `vos-refactor` skill clears markers one at a time;
`vos-pre-commit` blocks on any above 0.5.
