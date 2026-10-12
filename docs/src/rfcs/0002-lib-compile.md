---
status: implemented
created: 2026-05-09
implemented-in: issues/17-lib-compile
---

# 0002 — `lib:compile` — programmatic single-file executable bundler

## Summary

`lib:compile` exposes `bundle(opts)` to Lua scripts — packages a source file into a self-contained executable. Appends source as payload to running `neoc` binary + 20-byte trailer. On launch, binary detects trailer → reads embedded source → executes directly. No runtime install required on target.

## Motivation

LLM agents and automation produce artefacts needing distribution without requiring recipients to install `neoc`. Single `compile.bundle()` call in `build.lua` = entire distribution story.

Validated in spike (branch `issues/17-lib-compile`). Prior-art survey compared Go/Rust/Zig embedding against Node.js SEA, Bun `bun build --compile`, Deno `deno compile`:

- **Single-command UX** (Bun/Deno model) correct — multi-step pipelines (Node SEA) universally rejected
- **Programmatic-first** (Bun's `Bun.build`) beats CLI flags — users always reach for programmatic form
- **Binary size** at stub + source level; neoc stub order-of-magnitude smaller than JS runtimes (target: < 15 MB stripped)

Append-to-stub = correct v1: no build-time embedding, works with any runtime-supplied source, no toolchain dependency beyond running `neoc`.

## Detailed design

### Lua API

```lua
local compile = require("lib:compile")

-- Bundle main.lua into a self-contained executable.
local ok, err = compile.bundle({
    entrypoint = "main.lua",  -- required: path to the Lua source file
    output     = "my-app",     -- required: output path for the bundled binary
})

if not ok then
    error("bundle failed: " .. err)
end
```

Returns `(true, nil)` on success, `(nil, error_string)` on failure per `lib:*` tuple-error convention (ADR-0005).

Both fields required. No defaults — silent defaults → surprising results.

### Trailer format

20-byte trailer appended after source payload:

```
Offset  Size  Field    Description
──────  ────  ───────  ────────────────────────────────────────────────────────
0       7     magic    b"NEOC_BL" — identifies a neoc bundled binary
7       1     version  0x01 — trailer format version
8       8     offset   u64 little-endian — byte offset of payload start
16      4     length   u32 little-endian — byte length of payload
────────────
Total: 20 bytes
```

Startup: seek `EOF − 20`, read trailer candidate. Magic match + recognised version → validate bounds → read + execute payload. No magic → normal `neoc` launcher.

Version `0x01` only defined version. Unrecognised version = hard error (future `neoc` with incompatible trailer).

Payload cap: 4 GiB (u32 length). Not a practical constraint for Lua.

### Bundle process

1. Read source bytes from `entrypoint`
2. Resolve running `neoc` via `std::env::current_exe()` as stub
3. Build output: `stub_bytes || source_bytes || trailer`
4. Write atomically (full build in memory before write)
5. Set executable bit on Unix (`0o111`)

`bundle()` callable only from running `neoc` — bundler is runtime capability, not standalone tool.

### Payload: raw source (v1)

v1 embeds raw Lua source. Bytecode embedding rejected: LuaJIT does not verify bytecode, so a crafted chunk corrupts memory and escapes the sandbox. All script entry points load text only (`crates/neoc-vm/src/lua/sandbox.rs`).

### Cross-compilation (v1)

v1 = current platform only. Running `neoc` = stub; no foreign-target injection. Cross-compilation via pre-published stubs (Bun/Deno download-target model) planned as follow-on in issue #17.

### macOS signing

Bundled binaries inherit stub's code signature. On macOS:

- **Development**: re-sign ad-hoc after bundling:
  ```sh
  codesign --force --sign - my-app
  ```
  Satisfies Gatekeeper locally; not notarized, quarantined on other Macs.

- **Distribution**: notarization out of scope for v1. Workaround: `xattr -d com.apple.quarantine my-app`. Matches Bun/Deno approach.

## Drawbacks

- **No cross-compilation.** Workaround: build on target platform or in CI.
- **Raw source readable** in output binary. Bytecode not an option (sandbox escape).
- **Stub growth = output growth.** Keep stripped release < 15 MB.
- **`current_exe()` requirement.** Must run from `neoc` process.
- **macOS quarantine.** Ad-hoc signed downloads quarantined. Notarization deferred.

## Alternatives

| Alternative | Verdict |
|---|---|
| `neoc compile` CLI subcommand | Rejected — programmatic API correct contract (Bun precedent) |
| Bytecode pre-compilation | Rejected — unverified LuaJIT bytecode escapes the sandbox |
| `include_bytes!()` at build time | N/A — entry script user-supplied at runtime |
| Cross-compilation via `cargo-zigbuild` | Deferred — requires pre-published stubs |
| Cosmopolitan APE | Experimental Rust integration. Parked. |
| Directory-root model | N/A — current-exe stub only correct approach for append-to-binary |

## Open questions

None. v1 API settled; implementation merged in `!3`.

## Implementation notes

- `src/lua/lib/compile.rs` — module; exports `pub fn module(lua)` and `pub fn detect_embedded_payload_from(exe)`. Detection called from `main.rs` before CLI parsing.
- `src/lua/lib/mod.rs` — `pub mod compile` added alphabetically
- `src/lua/modules.rs` — `lib:compile` registered
- `src/main.rs` — startup: `detect_embedded_payload_from(current_exe)` → `Some(source)` → execute + exit; else normal CLI
- `tests/lib/compile.test.lua` — integration test scaffold
- `Cargo.toml` — `tempfile` added for unit test helpers

Go/no-go: **Go**. POC validated append-to-stub end-to-end. Estimate 2–4 days; landed within window.
