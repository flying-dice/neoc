---
status: draft
created: 2026-05-09
implemented-in: issues/17-lib-compile
---

# 0002 — `lib:compile` — programmatic single-file executable bundler

## Summary

`lib:compile` exposes a `bundle(opts)` function to Luau scripts, allowing a `build.luau` to package a Luau source file into a self-contained executable. The bundler appends the source as a payload to the running `neoc` binary, followed by a 20-byte identifying trailer. On next launch the binary detects the trailer, reads the embedded source, and executes it directly — no runtime installation required on the target machine.

## Motivation

LLM agents and automation scripts produce artefacts (screenshots, reports, processed data) that need to be shared without requiring the recipient to install `neoc` or any scripting runtime. A single `compile.bundle({ entrypoint = "main.luau", output = "my-app" })` call in a `build.luau` is the entire distribution story.

This approach was validated in the spike for [flying-dice/neoc#17](https://gitlab.beluga-sirius.ts.net/flying-dice/neoc/-/issues/17). The prior-art survey documented in that issue compared Go/Rust/Zig compile-time embedding against Node.js SEA, Bun `bun build --compile`, and Deno `deno compile`. Key lessons:

- **Single-command UX** (Bun, Deno model) is correct — multi-step pipelines (Node SEA) are universally rejected by users.
- **Programmatic-first** (Bun's `Bun.build` API) beats CLI flags — users always reach for the programmatic form in practice.
- **Binary size** should be kept at the stub + source level; the neoc stub is an order of magnitude smaller than any JS runtime (target: < 15 MB stripped).

The append-to-stub model is the correct v1 choice: it requires no build-time embedding, works with any source file supplied at runtime, and has no toolchain dependency beyond the running `neoc` binary itself.

## Detailed design

### Luau API

```lua
local compile = require("lib:compile")

-- Bundle main.luau into a self-contained executable.
local ok, err = compile.bundle({
    entrypoint = "main.luau",  -- required: path to the Luau source file
    output     = "my-app",     -- required: output path for the bundled binary
})

if not ok then
    error("bundle failed: " .. err)
end
```

`compile.bundle` returns `(true, nil)` on success and `(nil, error_string)` on failure, following the `lib:*` tuple-error convention (ADR-0005).

Both `entrypoint` and `output` are required. Omitting either is a hard error — there is no reasonable default for either field, and silent defaults would produce surprising results.

### Trailer format

The 20-byte trailer is appended immediately after the source payload:

```
Offset  Size  Field    Description
──────  ────  ───────  ─────────────────────────────────────────────────────
0       7     magic    b"NEOC_BL" — identifies a neoc bundled binary
7       1     version  0x01 — trailer format version
8       8     offset   u64 little-endian — byte offset of payload start
16      4     length   u32 little-endian — byte length of payload
──────  ────
Total: 20 bytes
```

On startup `neoc` seeks `EOF − 20` and reads the trailer candidate. If the magic bytes match and the version is recognised, the bounds are validated and the payload is read and executed. If no magic is found the binary behaves as a normal `neoc` launcher.

Version `0x01` is the only defined version. An unrecognised version byte is a hard error (the binary may have been produced by a future `neoc` version with an incompatible trailer).

The payload cap is 4 GiB (u32 length field). This is not a practical constraint for Luau scripts.

### Bundle process

1. Read source bytes from `entrypoint`.
2. Resolve the running `neoc` binary via `std::env::current_exe()` as the stub.
3. Build the output in memory: `stub_bytes || source_bytes || trailer`.
4. Write the output atomically (full build in memory before writing).
5. Set the executable bit on Unix (`0o111`).

The current-exe approach means `bundle()` can only be called from a running `neoc` instance. This is the intended use case — the bundler is a capability of the runtime, not a standalone tool.

### Payload: raw source (v1 scope)

v1 embeds raw Luau source bytes. Bytecode pre-compilation would reduce startup time by skipping the Luau compiler at launch, but requires `luau_compile` bindings and measurably complicates the implementation. Deferred to a follow-on issue.

### Cross-compilation (v1 scope)

v1 produces a binary for the current platform only. The running `neoc` binary is always the stub; there is no mechanism to inject a foreign-target stub. Cross-compilation via pre-published target stubs (matching Bun/Deno's download-target model) is the planned follow-on approach and is documented as an open acceptance-criteria item in issue #17.

### macOS signing strategy

Binaries produced by `compile.bundle` inherit the code signature (or lack thereof) of the `neoc` stub. On macOS:

- **Development / local testing**: re-sign with ad-hoc signature after bundling:
  ```sh
  codesign --force --sign - my-app
  ```
  Ad-hoc signing satisfies Gatekeeper on the machine that signs but is not notarized and will be quarantined on other Macs.

- **Distribution**: Apple notarization is a separate workstream, explicitly out of scope for v1. The `neoc` release binary should be notarized before `compile.bundle` is recommended for producing distributable macOS applications. Until then, distribution via direct download with documented user instructions (`xattr -d com.apple.quarantine my-app`) is the pragmatic workaround.

This strategy matches the approach taken by Bun and Deno for their compiled outputs — both document ad-hoc signing for development and leave notarization to downstream distribution pipelines.

## Drawbacks

- **No cross-compilation.** A developer on macOS cannot produce a Linux binary. Workaround: build on the target platform or in CI.
- **Payload is raw source.** The Luau source is readable in the output binary with standard tools. If source confidentiality matters, this is a gap. Bytecode compilation is the follow-on.
- **Stub growth = output growth.** As `neoc` grows, so does every bundled binary. The stripped release binary should be kept below 15 MB.
- **`current_exe()` requirement.** `compile.bundle` must be called from within a running `neoc` process — it cannot be used from a native Rust binary or a detached build tool.
- **macOS quarantine.** Ad-hoc signed binaries distributed via download will be quarantined on recipient Macs. Documented workaround only; notarization deferred.

## Alternatives

| Alternative | Verdict |
|---|---|
| `neoc compile` CLI subcommand | Rejected per CODEOWNER direction. Programmatic API is the right contract — users always reach for the programmatic form in practice (Bun precedent). |
| Bytecode pre-compilation (v1) | Deferred. Adds complexity; not needed for the target use case (automation scripts are small). |
| `include_bytes!()` at build time | Not applicable — entry script is user-supplied at runtime. |
| Cross-compilation via `cargo-zigbuild` | Deferred. Requires pre-published stubs; a separate follow-on workstream. |
| Cosmopolitan APE | Experimental Rust integration. Parked — potential future option for zero-install on Windows. |
| Directory-root model (Go/Rust `ServeDir` analogy) | Not applicable to a bundler, but included for completeness: the current-exe stub model is the only correct approach for append-to-binary bundling. |

## Open questions

None. The v1 API surface is settled; implementation pending merge.

## Implementation notes

- `src/lua/lib/compile.rs` — module implementation; exports `pub fn module(lua)` and `pub fn detect_embedded_payload_from(exe)`. The detection function is called from `main.rs` before CLI argument parsing.
- `src/lua/lib/mod.rs` — `pub mod compile` added alphabetically.
- `src/lua/modules.rs` — `lib:compile` registered.
- `src/main.rs` — startup detection: if `detect_embedded_payload_from(current_exe)` returns `Some(source)`, execute the source and exit; otherwise fall through to normal CLI handling.
- `tests/lib/compile.test.luau` — integration test scaffold.
- `Cargo.toml` — `tempfile` crate added for unit test helpers.

The go/no-go recommendation for active implementation was **Go**. The POC validated the append-to-stub approach end-to-end; the effort estimate for bringing the spike to a shippable `lib:compile` module was two to four days of focused implementation work. The implementation landed within that window.
