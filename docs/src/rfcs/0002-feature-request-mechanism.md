---
status: draft
created: 2026-05-05
implemented-in:
---

# 0002 — Built-in feature request mechanism for LLM callers

> [!WARNING]
> This RFC is `draft`. It is under refinement and is **not** the contract.

## Summary

Add a first-class channel through which an executing script can emit a structured capability gap report — a **feature request** — that survives the process and is observable by the caller. The mechanism is a `std:feedback` module exposing a single `request(t)` function; the runtime collects these into a newline-delimited JSON file at `~/.neoc/feature-requests.jsonl` (overridable via environment variable). A companion CLI flag `--feature-request` bypasses script execution entirely for one-liner requests. Neither variant requires network access, external services, or elevated permissions.

## Motivation

When an LLM agent uses `neoc path/to/script.luau` as its sole tool and hits a capability gap, it currently has two options: work around the gap silently (reaching for `bash`, `curl`, or Python) or fail. Both outcomes are invisible to the maintainers of the runtime. The actual demand signal for which modules to build next comes from guesswork or from watching what agents reach for on the other side of the process boundary.

This RFC proposes closing that loop. Every time an agent identifies a missing capability, it can record that identification as a structured artefact attached to the `neoc` invocation. The artefact persists locally. The maintainers can inspect the log, count frequencies, and build the highest-demand modules first. The backlog writes itself from real usage.

The three concrete scenarios that are currently awkward or impossible:

1. **Require falls through.** An agent calls `require("lib:regex")`, catches the error, implements a workaround using Lua patterns, and moves on. The missing module is never recorded.
2. **Capability exists but is insufficient.** An agent calls `std:fs.read_to_string`, discovers there is no streaming API for large files, falls back to shell piping. No signal is generated.
3. **Caller-side harness.** A developer driving a fleet of agents wants a structured daily report of "what did the agents need that neoc didn't have?" There is currently no artefact to aggregate.

### Peer runtime survey

Before proposing anything, it is worth checking whether this pattern already exists in runtimes agents use today and how they handle it.

**Node.js / npm.** No built-in mechanism. Missing APIs are discovered through community issue trackers; detection is entirely out-of-band. The feedback channel is the public internet.

**Deno.** No built-in mechanism either. Deno does provide `--allow-*` permission flags that surface capability boundaries explicitly, but those are security gates, not feedback channels. When a permission is denied, the error is structured and names the resource — the agent can detect the denial at runtime — but there is no first-class path to record "I needed this, I was denied" beyond catching the error and writing to a file manually.

**Bun.** Same picture as Node. No built-in capability gap reporting. The shell integration (`Bun.shell`) makes it trivial to reach for the host shell as an escape hatch, which is precisely the silent workaround problem this RFC addresses.

**Lua 5.4 / PUC Lua.** No equivalent concept. The standard library is intentionally minimal; gap-filling is left to LuaRocks. Missing modules surface as `require` errors; whether those errors are observed is entirely up to the caller.

**Luau (Roblox).** Luau's `getfenv`/`setfenv` removal and its capability model are security-focused, not feedback-focused. The Roblox platform has an internal feature request forum, but nothing accessible from a running script.

**Conclusion.** No peer runtime has a built-in, structured, local-first capability gap reporting mechanism. This feature has no direct model to mirror (design principle 3 — "mirror Rust first" — does not apply here because there is nothing to mirror). Principles 1 and 2 therefore drive the design: **no surprises** (the mechanism should feel like a normal module call) and **no footguns** (the mechanism must not silently lose requests, must not block execution, and must not touch the network without explicit opt-in).

### Why not a global hook on `require` failure?

Automatic detection on `require` failure would capture the most common gap — a missing module — without requiring the agent to write any extra code. It is rejected here for three reasons:

1. **Not all `require` failures are capability gaps.** An agent doing `pcall(require, "lib:json")` to probe for an optional dependency is a normal pattern; recording that probe as a feature request generates noise that erodes the signal.
2. **The reason matters more than the event.** A structured request carries `operation`, `reason`, and `fallback` — context that makes the request actionable. A hook on `require` captures none of that.
3. **Opt-in is the right default for a feedback mechanism.** An agent that never calls `std:feedback.request` generates no requests. That silence is meaningful: it means the agent found everything it needed, or it chose not to report what it didn't.

The automatic hook is available as a future opt-in (see [Open questions](#open-questions)).

## Detailed design

### Module: `std:feedback`

```lua
local feedback = require("std:feedback")

-- Emit a feature request.
-- All fields except `module` are optional.
local ok, err = feedback.request({
    module    = "lib:regex",              -- required: the gap identifier
    operation = "match, replace, split",  -- what the agent needed to do
    reason    = "Lua patterns cannot handle non-greedy quantifiers in URL extraction",
    fallback  = "used std:process to shell out to grep -oP",
})
-- Returns (true, nil) on success.
-- Returns (nil, error_string) if the write fails (disk full, permissions, etc.).
```

The function is non-blocking. It appends one line to the configured sink and returns immediately. A write failure is surfaced as a `(nil, err)` return — consistent with the rest of the `neoc` API — but does **not** raise an error. The agent decides whether a failed feedback write is fatal to its own logic.

#### Record schema

Each call appends one JSON line to the sink. The schema is stable and additive — future fields may be added but existing fields will not be renamed or removed.

```json
{
  "version":   1,
  "ts":        "2026-05-05T14:23:01.456Z",
  "module":    "lib:regex",
  "operation": "match, replace, split",
  "reason":    "Lua patterns cannot handle non-greedy quantifiers in URL extraction",
  "fallback":  "used std:process to shell out to grep -oP",
  "session":   "a3f1c9d2e4b6f801"
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `version` | integer | always | Schema version. Currently `1`. |
| `ts` | ISO 8601 string | always | UTC timestamp at the moment of the call. |
| `module` | string | always | The requested capability identifier (e.g. `lib:regex`, `lib:csv`). |
| `operation` | string | optional | What the caller needed to do with the module. |
| `reason` | string | optional | Why the built-in alternatives were insufficient. |
| `fallback` | string | optional | What the caller did instead. |
| `session` | string | always | A per-process random hex identifier (16 chars / 64-bit), stable for the lifetime of one `neoc` invocation. Groups requests from the same session. |

The `session` field enables aggregation: ten requests from the same agent run share a `session` id, so a post-processing tool can count distinct sessions per `module` rather than raw request counts.

#### Sink configuration

| Priority | Source | Value |
|---|---|---|
| 1 | Env var `NEOC_FEEDBACK_SINK` | Absolute path to the JSONL file |
| 2 | Default | `~/.neoc/feature-requests.jsonl` |

The directory is created on first use if it does not exist. No migration is required — the file is append-only JSONL; new records from newer versions coexist with old records without issue.

To route requests to stdout (for the caller's harness to capture), set `NEOC_FEEDBACK_SINK=/dev/stdout` (Unix) or pipe via a wrapper script on Windows.

### CLI flag: `--feature-request`

For one-liner requests without a script:

```bash
neoc --feature-request "lib:regex: need non-greedy quantifiers for URL extraction from markdown"
```

The argument is parsed as `<module-id>: <reason>`. The split key is the two-character sequence `: ` (ASCII `0x3A` followed by ASCII `0x20`); the split occurs at the **first** occurrence. Everything before that sequence is the `module-id`; everything after is the `reason`. Examples: `lib:regex: need non-greedy quantifiers` → module `lib:regex`, reason `need non-greedy quantifiers`. A colon not followed by a space (e.g. as part of a sub-path like `std:fs.read`) is part of the module identifier and does not trigger a split. If no `: ` sequence is present, the entire string is treated as `module` with an empty `reason`. A trailing `: ` with nothing after it produces an empty `reason` string — this is recorded as-is, not treated as an error. All other optional fields are absent. The process exits 0 after recording.

This is a convenience path for interactive developer use and for callers that prefer not to embed Lua for a purely administrative action.

### Deduplication

Deduplication is intentionally **not** the runtime's job. The runtime appends every call faithfully. Deduplication is a read-time concern, handled by whatever tool processes the JSONL file. This keeps the write path simple, avoids introducing state that must survive crashes, and lets the consumer choose its deduplication key (module + session? module + reason prefix? exact record hash?).

A reference aggregation script will be included in `scripts/` as part of the implementation PR. It groups by `module`, counts distinct `session` values, and emits a ranked table.

### Rust-side layout

```
src/lua/std/
  feedback.rs      -- the std:feedback module
  mod.rs           -- add feedback to module registration
src/
  config.rs        -- new file: NEOC_FEEDBACK_SINK resolution, session id generation
  main.rs          -- --feature-request flag handling
```

`config.rs` does not currently exist and must be created. It will hold feedback sink path resolution and session id generation. `feedback.rs` is a new module following the standard `pub fn module(lua: &Lua) -> mlua::Result<Table>` convention.

### Error semantics

`feedback.request` never panics and never raises a Lua error. All failure modes return `(nil, err_string)`:

| Failure | `err_string` prefix |
|---|---|
| `module` field missing or not a string | `"feedback.request: 'module' must be a non-empty string"` |
| Sink directory creation failed | `"feedback.request: could not create sink directory: …"` |
| File open/write failed | `"feedback.request: could not write to sink: …"` |

### Acceptance criteria

1. With `NEOC_FEEDBACK_SINK` set to a temp path, `require("std:feedback").request({ module = "lib:regex" })` appends exactly one valid JSON line to that path and returns `(true, nil)`.
2. `require("std:feedback").request({})` returns `(nil, "feedback.request: 'module' must be a non-empty string")` and writes nothing.
3. A write failure (unwritable path via `NEOC_FEEDBACK_SINK`) returns `(nil, err)` and does not raise a Lua error or terminate the script.
4. Two calls in the same `neoc` process share the same `session` value. Two calls in separate `neoc` processes have distinct `session` values (guaranteed with high probability: 16-char hex / 64-bit id, birthday bound 50% collision at ~4 billion sessions).
5. `neoc --feature-request "lib:regex: need X"` exits 0 and appends one record with `module = "lib:regex"` and `reason = "need X"`.
6. Setting `NEOC_FEEDBACK_SINK=/tmp/test.jsonl` redirects all writes to that path.
7. All existing tests pass (the module is additive; nothing is removed).

## Drawbacks

- **Surface expansion.** A new module is added to `std:*`. The surface is small — one function — but it is there.
- **Sink file grows unbounded.** No rotation, no cap. A long-running fleet of agents could produce a large file. Rotation is out of scope here; the caller is responsible for periodic archival. This is the same contract as any append-only log.
- **Session id is process-scoped, not agent-scoped.** If a supervisor spawns multiple `neoc path/to/script.luau` calls for a single logical agent task, each invocation has its own `session`. Correlating across processes requires caller-side logic (e.g. passing a correlation id in `reason` or via a wrapper). This is a deliberate scope limit — cross-process identity is the caller's problem.
- **No automatic detection.** The mechanism requires explicit call sites. A script that does not call `feedback.request` generates no signal, even if it silently works around every gap it encounters. This is the correct default (see Motivation), but it means adoption depends on agent authors instrumenting their scripts.

## Alternatives

### Emit to stdout as structured JSON

Route requests to stdout rather than a file. The caller captures `neoc`'s stdout and parses the mixed script-output + feedback stream. Rejected because:

- `neoc path/to/script.luau` scripts routinely print to stdout as part of their normal operation. Multiplexing feedback records into that stream requires a framing convention (distinguishing feedback lines from script output) that adds complexity on both sides.
- The file-based sink is zero-coordination: the agent writes, the maintainer reads, no protocol negotiation required.
- `NEOC_FEEDBACK_SINK=/dev/stdout` gives the stdout path to any caller that genuinely wants it, without imposing it on everyone.

### Auto-record `require` failures

Intercept every failed `require` call and automatically log it as a feature request. Rejected for the reasons stated in Motivation. Available as a future opt-in via a module-level or CLI flag (see [Open questions](#open-questions)).

### GitHub issue auto-creation

Write requests directly to a GitHub issue tracker via the API. Rejected at this stage because:

- Requires network access and a credential — neoc is deliberately sandboxed and this is a significant permission expansion.
- Deduplication across issues is hard; GitHub's API is not designed for high-frequency write fan-in from many agent processes.
- The local JSONL file is a better staging layer: collect locally, promote to GitHub on a human-gated schedule.
- Nothing prevents the caller from reading the JSONL and creating issues downstream; that policy does not belong in the runtime.

### CLI-only (`--request-feature`), no Lua module

Provide only the CLI flag, not the `std:feedback` module. Rejected because the most important use case — an agent detecting a gap *inside a script* and recording it with full context before proceeding with its fallback — requires an in-script API. The CLI flag is a complement, not a substitute.

### Configurable backends (file / stdout / GitHub / webhook)

Make the sink pluggable at runtime. Rejected because the abstraction cost is not justified by the current use cases. The environment variable approach (set `NEOC_FEEDBACK_SINK` to any path) already covers the stdout, file, and pipe-to-webhook cases without any code change. A formal backend plugin model can be introduced in a follow-up RFC if real demand emerges.

## Open questions

1. **Opt-in automatic `require` monitoring.** Should a future flag (e.g. `NEOC_AUTO_FEEDBACK=1` or `--auto-feedback`) auto-record all `require` failures as feature requests, with an empty `reason` and `fallback`? This would lower the bar for passive signal collection without changing the default. Decision deferred — the explicit API ships first; the automatic hook is a follow-on if the explicit adoption proves too low.

2. **Structured module field.** `module` is currently a free string. Should the runtime validate that it follows the `ns:name` convention (e.g. `lib:regex`, `std:fs`)? Stricter validation catches typos at the cost of rejecting valid but unconventional identifiers (e.g. a request for a CLI flag or a build system feature). Leaning toward a warning on `stderr` for strings that do not match `[a-z]+:[a-z][a-z_.:]*` rather than a hard error.

3. **Windows sink path.** `~/.neoc/` expands differently on Windows (`%USERPROFILE%\.neoc\`). The implementation must handle `$HOME` resolution correctly. This is an implementation concern, not a design concern, but it should be tracked on the implementation issue.

4. **`feedback.list()` accessor.** Should the module expose a read path — `feedback.list()` — that returns the recorded requests as a Lua table? Useful for agents that want to inspect and surface their own gap log. Deferred to avoid scope creep in the first iteration; the JSONL file is readable by any script via `std:fs`.

5. **Namespace placement.** `std:feedback` is proposed as a `std:*` module, but it does not mirror any Rust `std` type — it is a neoc-specific meta-mechanism. `lib:feedback` would be equally valid under the namespace rules. The `std:*` placement is chosen because capability gap reporting is intrinsic to the runtime (not a gap-filler or vendored crate), but this should be confirmed before the implementation PR. If the CODEOWNER prefers `lib:feedback`, the API surface is identical — only the `require` path changes.

## Implementation notes

- The `session` id is a random 16-char hex string (64-bit) generated once at process start in the new `src/config.rs` and stored as a `LazyLock<String>` (or equivalent). It is passed into the feedback module at registration time.
- The JSONL append must be atomic at the line level: write the full JSON record plus `\n` in a single `write_all` call with `O_APPEND`. For local filesystems, a single `write_all` with `O_APPEND` is de-facto atomic for records of this size (< 512 bytes). This is not guaranteed on network filesystems (NFS, SMB). The disclaimer already present in this section covers that case.
- The `--feature-request` flag parsing in `main.rs` should run before engine initialisation — it does not need a Lua VM.
- `std:feedback` is a `std:*` module. It uses only `std` and `blessed.rs` dependencies (specifically `serde_json` for serialisation, which is already in the dependency tree). No new crate dependencies are required.
- The `scripts/aggregate-feedback.sh` (or `.py`) reference script is out of scope for the RFC and will be specced on the implementation issue.
