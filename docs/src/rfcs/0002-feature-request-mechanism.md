---
status: draft
created: 2026-05-05
implemented-in:
---

# 0002 — Built-in feature request mechanism for LLM callers

> [!WARNING]
> This RFC is `draft`. Under refinement — **not** the contract.

## Summary

First-class channel for scripts to emit structured capability gap reports. `std:feedback` module exposes `request(t)` → appends JSONL to `~/.neoc/feature-requests.jsonl` (overridable via `NEOC_FEEDBACK_SINK`). Companion CLI flag `--feature-request` bypasses script execution for one-liner requests. No network, no services, no elevated permissions.

## Motivation

Agent hits capability gap → two options today: silent workaround (bash/curl/Python) or fail. Both invisible to runtime maintainers. Demand signal for next modules = guesswork.

This RFC closes the loop. Agent records structured gap artefact → persists locally → maintainers inspect, count frequencies, build highest-demand modules first. Backlog writes itself from real usage.

Scenarios currently awkward/impossible:

1. **Require falls through.** Agent calls `require("lib:regex")`, catches error, implements workaround via Lua patterns. Missing module never recorded.
2. **Insufficient capability.** Agent calls `std:fs.read_to_string`, discovers no streaming API for large files, falls back to shell piping. No signal generated.
3. **Fleet aggregation.** Developer driving agent fleet wants structured daily report of gaps. No artefact to aggregate.

### Peer runtime survey

**Node.js / npm.** No built-in mechanism. Gaps discovered via issue trackers; detection entirely out-of-band.

**Deno.** No built-in mechanism. `--allow-*` flags surface capability boundaries but are security gates, not feedback channels. Denied permissions produce structured errors naming the resource — but no first-class path to record "needed X, denied" beyond manual file writes.

**Bun.** Same as Node. `Bun.shell` makes host shell escape trivial — precisely the silent workaround problem addressed here.

**Lua 5.4 / PUC Lua.** No equivalent. Gap-filling left to LuaRocks. Missing modules surface as `require` errors; observation is caller's problem.

**Luau (Roblox).** Security-focused capability model, not feedback-focused. Internal feature request forum exists but nothing accessible from running scripts.

**Conclusion.** No peer runtime has built-in, structured, local-first capability gap reporting. Nothing to mirror (principle 3 N/A). Principles 1+2 drive design: **no surprises** (feels like normal module call) and **no footguns** (never silently lose requests, never block execution, never touch network without opt-in).

### Why not a global hook on `require` failure?

Rejected for three reasons:

1. **Not all failures are gaps.** `pcall(require, "lib:json")` probing optional dependency = normal pattern; recording as request → noise erodes signal.
2. **Reason matters more than event.** Structured request carries `operation`, `reason`, `fallback` — actionable context. Hook captures none.
3. **Opt-in is correct default.** No calls → no requests. Silence = agent found everything needed or chose not to report.

Automatic hook available as future opt-in (see [Open questions](#open-questions)).

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

Non-blocking. Appends one line to configured sink, returns immediately. Write failure surfaced as `(nil, err)` — consistent with rest of `neoc` API — does **not** raise error. Agent decides whether failed feedback write is fatal to its logic.

#### Record schema

Each call appends one JSON line. Schema is stable and additive — future fields added, existing fields never renamed/removed.

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
| `ts` | ISO 8601 string | always | UTC timestamp at moment of call. |
| `module` | string | always | Requested capability identifier (e.g. `lib:regex`, `lib:csv`). |
| `operation` | string | optional | What caller needed to do with the module. |
| `reason` | string | optional | Why built-in alternatives were insufficient. |
| `fallback` | string | optional | What caller did instead. |
| `session` | string | always | Per-process random hex identifier (16 chars / 64-bit), stable for lifetime of one `neoc` invocation. Groups requests from same session. |

`session` enables aggregation: ten requests from same agent run share id → post-processing counts distinct sessions per `module` rather than raw counts.

#### Sink configuration

| Priority | Source | Value |
|---|---|---|
| 1 | Env var `NEOC_FEEDBACK_SINK` | Absolute path to JSONL file |
| 2 | Default | `~/.neoc/feature-requests.jsonl` |

Directory created on first use if absent. No migration — append-only JSONL; new records coexist with old.

Route to stdout: `NEOC_FEEDBACK_SINK=/dev/stdout` (Unix) or pipe via wrapper on Windows.

### CLI flag: `--feature-request`

One-liner requests without a script:

```bash
neoc --feature-request "lib:regex: need non-greedy quantifiers for URL extraction from markdown"
```

Parsed as `<module-id>: <reason>`. Split key = `: ` (first occurrence of `0x3A 0x20`). Before = `module-id`; after = `reason`. Colon not followed by space (e.g. `std:fs.read`) does not trigger split. No `: ` present → entire string = `module`, empty `reason`. Trailing `: ` with nothing after → empty `reason` (recorded as-is, not error). All other optional fields absent. Process exits 0 after recording.

Convenience path for interactive developer use and callers preferring not to embed Lua for administrative actions.

### Deduplication

Intentionally **not** the runtime's job. Runtime appends every call faithfully. Deduplication = read-time concern, handled by consumer. Keeps write path simple, avoids crash-sensitive state, lets consumer choose dedup key (module + session? module + reason prefix? exact hash?).

Reference aggregation script included in `scripts/` with implementation PR — groups by `module`, counts distinct `session` values, emits ranked table.

### Rust-side layout

```
src/lua/std/
  feedback.rs      -- the std:feedback module
  mod.rs           -- add feedback to module registration
src/
  config.rs        -- new file: NEOC_FEEDBACK_SINK resolution, session id generation
  main.rs          -- --feature-request flag handling
```

`config.rs` must be created. Holds feedback sink path resolution + session id generation. `feedback.rs` follows standard `pub fn module(lua: &Lua) -> mlua::Result<Table>` convention.

### Error semantics

`feedback.request` never panics, never raises Lua error. All failures return `(nil, err_string)`:

| Failure | `err_string` prefix |
|---|---|
| `module` field missing or not a string | `"feedback.request: 'module' must be a non-empty string"` |
| Sink directory creation failed | `"feedback.request: could not create sink directory: …"` |
| File open/write failed | `"feedback.request: could not write to sink: …"` |

### Acceptance criteria

1. `NEOC_FEEDBACK_SINK` set to temp path → `require("std:feedback").request({ module = "lib:regex" })` appends exactly one valid JSON line, returns `(true, nil)`.
2. `require("std:feedback").request({})` → returns `(nil, "feedback.request: 'module' must be a non-empty string")`, writes nothing.
3. Unwritable path via `NEOC_FEEDBACK_SINK` → returns `(nil, err)`, no Lua error raised, script continues.
4. Two calls same process → same `session`. Two calls separate processes → distinct `session` (64-bit; birthday bound 50% collision at ~4B sessions).
5. `neoc --feature-request "lib:regex: need X"` → exits 0, appends record with `module = "lib:regex"`, `reason = "need X"`.
6. `NEOC_FEEDBACK_SINK=/tmp/test.jsonl` → all writes go to that path.
7. All existing tests pass (module is additive; nothing removed).

## Drawbacks

- **Surface expansion.** New `std:*` module. Small surface (one function) but present.
- **Unbounded sink.** No rotation, no cap. Long-running fleet → large file. Rotation out of scope; caller responsible for archival. Same contract as any append-only log.
- **Session = process-scoped, not agent-scoped.** Supervisor spawning multiple `neoc` calls per logical task → each invocation gets own session. Cross-process correlation = caller's problem (pass correlation id in `reason` or via wrapper). Deliberate scope limit.
- **No automatic detection.** Requires explicit call sites. Script not calling `feedback.request` generates no signal even if it silently works around gaps. Correct default (see Motivation) but adoption depends on agent authors instrumenting scripts.

## Alternatives

### Emit to stdout as structured JSON

Rejected:
- Scripts routinely print to stdout → multiplexing feedback requires framing convention → complexity on both sides.
- File-based sink = zero-coordination: agent writes, maintainer reads, no protocol negotiation.
- `NEOC_FEEDBACK_SINK=/dev/stdout` available for callers that want it without imposing on everyone.

### Auto-record `require` failures

Rejected for reasons in Motivation. Available as future opt-in via module-level or CLI flag (see [Open questions](#open-questions)).

### GitHub issue auto-creation

Rejected:
- Requires network + credential → significant permission expansion for sandboxed runtime.
- Deduplication across issues hard; GitHub API not designed for high-frequency write fan-in from many agents.
- Local JSONL = better staging layer: collect locally, promote on human-gated schedule.
- Caller can read JSONL and create issues downstream; policy doesn't belong in runtime.

### CLI-only (`--request-feature`), no Lua module

Rejected. Most important use case = agent detecting gap *inside script* with full context before proceeding with fallback → requires in-script API. CLI flag = complement, not substitute.

### Configurable backends (file / stdout / GitHub / webhook)

Rejected. Abstraction cost not justified by current use cases. Env var approach (set `NEOC_FEEDBACK_SINK` to any path) covers stdout, file, and pipe-to-webhook without code change. Formal backend plugin model = follow-up RFC if real demand emerges.

## Open questions

1. **Opt-in auto `require` monitoring.** Future flag (`NEOC_AUTO_FEEDBACK=1` / `--auto-feedback`) auto-records all `require` failures with empty `reason`/`fallback`? Lowers bar for passive signal collection without changing default. Decision deferred — explicit API ships first; automatic hook follows if explicit adoption too low.

2. **Structured module field.** `module` is free string. Validate `ns:name` convention? Stricter validation catches typos at cost of rejecting unconventional identifiers (e.g. CLI flag request). Leaning toward warning on `stderr` for strings not matching `[a-z]+:[a-z][a-z_.:]*` rather than hard error.

3. **Windows sink path.** `~/.neoc/` expands differently on Windows (`%USERPROFILE%\.neoc\`). Implementation must handle `$HOME` resolution correctly. Implementation concern, not design concern — track on implementation issue.

4. **`feedback.list()` accessor.** Expose read path returning recorded requests as Lua table? Useful for agents inspecting own gap log. Deferred to avoid scope creep; JSONL readable via `std:fs`.

5. **Namespace placement.** `std:feedback` does not mirror Rust `std` type — neoc-specific meta-mechanism. `lib:feedback` equally valid. `std:*` chosen because capability gap reporting is intrinsic to runtime (not gap-filler or vendored crate). Confirm before implementation PR. If CODEOWNER prefers `lib:feedback`, API surface identical — only `require` path changes.

## Implementation notes

- `session` id: random 16-char hex (64-bit), generated once at process start in `src/config.rs`, stored as `LazyLock<String>`. Passed into feedback module at registration.
- JSONL append must be line-atomic: full JSON record + `\n` in single `write_all` with `O_APPEND`. De-facto atomic for records < 512 bytes on local filesystems. Not guaranteed on NFS/SMB (disclaimer covers this).
- `--feature-request` parsing in `main.rs` runs before engine init — no Lua VM needed.
- `std:feedback` uses only `std` + `blessed.rs` deps (`serde_json` for serialisation, already in tree). No new crate dependencies.
- `scripts/aggregate-feedback.sh` (or `.py`) reference script out of scope for RFC — specced on implementation issue.
