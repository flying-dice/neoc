# Contributing

This guide is the mechanical walkthrough — the order to do things in, where files go, and what to run before opening a merge request. It deliberately points at [STYLEGUIDE.md](STYLEGUIDE.md) and [README.md](README.md) for the conventions and the higher-level rationale rather than restating them.

## Before you start

- Rust toolchain (stable; `cargo` on `PATH`).
- Read the [Design principles](README.md#design-principles) section of the README — these four rules (no surprises, no footguns, mirror Rust, supplement with scripting ergonomics) govern every API decision and are non-negotiable.
- Read [STYLEGUIDE.md](STYLEGUIDE.md) once — it covers module shape, return types, error prefixes, resource handles, allowed dependencies, constructor naming, the namespace model (`std:*` / `lib:*` / `vnd:*`), and the Luau test conventions. The rules there are binding.
- Read the [Module workflow](README.md#module-workflow) section of the README — it describes the two-tier flow (isolated then integrated) the steps below implement.

## Adding a new module

The example below threads through as if you were adding a `foo` module. The first decision is the namespace — see [STYLEGUIDE.md → Module namespaces](STYLEGUIDE.md#module-namespaces). Once you've picked one, every step that mentions `<ns>:foo` resolves to it (e.g. `lib:foo`, `vnd:foo`, `std:foo`). Every step has a "where this is documented" pointer in case the convention is unclear.

### 1. Create the module file

`src/lua/lib/foo.rs`:

```rust
use mlua::{Lua, Table};

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("greet", lua.create_function(greet)?)?;
    Ok(t)
}

fn greet(_: &Lua, name: mlua::String) -> mlua::Result<String> {
    Ok(format!("hello, {}", name.to_str()?))
}
```

Conventions for the function shape, fallible vs infallible returns, error prefix format, and resource handles are in [STYLEGUIDE.md → Rust → mlua modules](STYLEGUIDE.md#mlua-modules). Allowed dependencies depend on the namespace — see [STYLEGUIDE.md → Allowed dependencies](STYLEGUIDE.md#allowed-dependencies).

### 2. Register the module

Add to `src/lua/lib/mod.rs` in two places — the `pub mod` declaration and the `install` body, under the right namespace section:

```rust
pub mod foo;
// ...
modules.insert("lib:foo".into(), foo::module(lua)?);  // or std:foo / vnd:foo
```

Keep both the `pub mod` list and the `modules.insert` calls within their namespace section alphabetical so diffs stay clean.

### 3. In-module Rust tests

Append a `#[cfg(test)] mod tests` block at the bottom of `src/lua/lib/foo.rs`. These cover the Rust core directly — no Lua boundary, fast feedback.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greet_basic() {
        let lua = Lua::new();
        let s = lua.create_string("world").unwrap();
        assert_eq!(greet(&lua, s).unwrap(), "hello, world");
    }
}
```

If a test needs the full engine (require resolution, other modules), use `crate::lua::sandbox::make_engine()`.

Run with `cargo test --lib`.

### 4. In-module Criterion benches

Bench code lives next to the module it exercises. Add a `pub fn benches(c: &mut Criterion)` at the bottom of `src/lua/lib/foo.rs`, gated behind the `bench` feature so it doesn't bloat normal builds. Three tiers per module — see [README.md → Bench tiers](README.md#bench-tiers) for what each one means and what regressions they catch.

```rust
#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use criterion::black_box;

    let lua = crate::lua::sandbox::make_engine().expect("engine");
    let payload = lua.create_string("world").unwrap();

    let mut g = c.benchmark_group("foo");

    // library — pure Rust crate, no mlua at all.
    g.bench_function("greet/library", |b| {
        b.iter(|| { let _ = black_box("hello, world"); });
    });

    // into_out_lua — identity-shape Rust fn, isolates marshalling cost.
    let ident: mlua::Function = lua
        .create_function(|_, s: mlua::String| Ok(s))
        .unwrap();
    g.bench_function("greet/into_out_lua", |b| {
        b.iter(|| { let _: mlua::String = ident.call(black_box(payload.clone())).unwrap(); });
    });

    // through_lua — full path the way users actually call it.
    let chunk: mlua::Function = lua
        .load(r#"return function(p) return (require("lib:foo")).greet(p) end"#)
        .eval()
        .unwrap();
    g.bench_function("greet/through_lua", |b| {
        b.iter(|| { let _: String = chunk.call(black_box(payload.clone())).unwrap(); });
    });

    g.finish();
}
```

Then add `lib::foo::benches` to the `criterion_group!` line at the bottom of `benches/lib.rs`.

Run with `cargo bench --bench lib foo` (the `bench` feature is auto-enabled by `[[bench]] required-features` in `Cargo.toml`, so no flag-juggling).

### 5. Luau integration test

Create `tests/<ns>/foo.test.luau` to exercise the public Lua API the way real scripts will, in the same engine as every other module. Test file shape, capture conventions, and coverage requirements are in [STYLEGUIDE.md → Luau](STYLEGUIDE.md#luau).

```lua
local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local foo = require("lib:foo")

describe("foo", function()
    describe("greet", function()
        test("basic case", function()
            expect(foo.greet("world")).toBe("hello, world")
        end)
    end)
end)
```

Run via the CLI: `cargo run -- tests/<ns>/foo.test.luau` (or all suites: `cargo run -- 'tests/**/*.test.luau'`).

### 6. Update the README

Add a row to the modules table in [README.md](README.md), under the namespace it belongs to. Keep the table alphabetical inside each namespace. If the module exposes anything non-obvious (resource handles, opt-in modes, env-var switches), give it its own short subsection.

## Adding to an existing module

Smaller surface — the same flow, in-place:

1. Add the function in `src/lua/lib/<name>.rs`.
2. Register it on the table in that module's `module()`.
3. Add `#[cfg(test)]` cases for it.
4. Extend the `pub fn benches` group with `library`, `into_out_lua`, `through_lua` rows for the new function.
5. Add `describe(...)` cases to `tests/<ns>/<name>.test.luau` (one per error condition; see the [STYLEGUIDE.md coverage requirement](STYLEGUIDE.md#coverage-requirement)).

## Local check matrix before opening an MR

```sh
cargo build                                 # builds clean
cargo test --lib                            # in-module Rust tests
cargo test --bin neoc                       # multi-worker subprocess tests
cargo run -- 'tests/**/*.test.luau'         # all Luau suites
cargo bench --bench lib <name>              # the group you touched, against a pinned baseline
cargo clippy --all-targets                  # no new warnings
cargo fmt --check                           # formatting
```

For a perf-sensitive change, pin a baseline before editing and compare after — see [README.md → Protecting against Criterion regressions](README.md#protecting-against-criterion-regressions).

## CI

The project is hosted on a self-hosted GitLab instance (`gitlab.beluga-sirius.ts.net/flying-dice/neoc`). GitLab CI runs automatically on push to `main` and on merge requests. See [issue #1](https://gitlab.beluga-sirius.ts.net/flying-dice/neoc/-/issues/1) for the pipeline definition — a `.gitlab-ci.yml` is the deliverable for that issue.

The intended pipeline stages are:

1. **Lint** — `cargo fmt --check` and `cargo clippy --all-targets` (warnings are errors via `RUSTFLAGS=-D warnings`).
2. **Test** — `cargo test --lib`, `cargo test --bin neoc`, and Luau integration tests on the self-hosted Linux runner.
3. **Bench** — Criterion benchmarks; manual/scheduled trigger only, results uploaded as artifacts.
4. **Release** — triggered on `v*` tags; builds a release binary and publishes a GitLab Release.

All lint and test jobs must pass before a merge request can be merged.

### Branching

Work branches follow the pattern enforced by the kanban workflow (see [CLAUDE.md](CLAUDE.md)):

- Implementation: `issues/{id}-{slug}` — e.g. `issues/6-std-task`
- Design artefacts: `design/{id}-{slug}` — e.g. `design/23-std-env`

### Releasing

Push a version tag to trigger the release pipeline:

```sh
git tag v0.1.0
git push origin v0.1.0
```

The pipeline builds a release binary, packages it as `neoc-x86_64-unknown-linux-gnu.tar.gz`, and creates a [GitLab Release](https://gitlab.beluga-sirius.ts.net/flying-dice/neoc/-/releases) with auto-generated release notes. The install scripts (`scripts/install.sh`, `scripts/install.ps1`) download from these release assets.

## MR expectations

- One module or one well-scoped change per merge request. Don't fold drive-by refactors into a feature MR.
- The commit message and MR description should say *why*, not just *what*. The diff already shows what.
- Update [README.md](README.md) and any relevant subsection in the same MR — don't leave docs to a follow-up.
- New crates require explicit sign-off; see the dependency rule in [STYLEGUIDE.md](STYLEGUIDE.md#allowed-dependencies) before adding anything to `Cargo.toml`. The `vnd:*` namespace exists specifically so that when a vendored crate IS the module, the rule is "match the crate name."
- If a benchmark regresses, the MR description should call it out and explain whether it's intentional (a deliberate trade for some other gain) or load-bearing for a follow-up.
- Apply the correct `workflow::*` label when opening the MR (`workflow::Implementation`) and update it to `workflow::Review` once the MR is ready for CODEOWNER review.

## When in doubt

- For module shape / return types / errors / namespacing: [STYLEGUIDE.md](STYLEGUIDE.md).
- For *why* the trade-offs are what they are: [README.md → Design philosophy](README.md#design-philosophy) and [What it's good at / not so good at](README.md#what-its-good-at).
- For testing and benching mechanics: this file plus [README.md → Module workflow](README.md#module-workflow).
