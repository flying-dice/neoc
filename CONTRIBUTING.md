# Contributing

Mechanical walkthrough — order, file locations, pre-PR checks. Points to [STYLEGUIDE.md](STYLEGUIDE.md) and [README.md](README.md) for conventions and rationale.

## Before you start

- `rustup` + `cargo` on `PATH`. Toolchain pinned in [rust-toolchain.toml](rust-toolchain.toml) — rustup auto-installs it; don't use `+stable`.
- Bumping the toolchain is its own PR: edit `rust-toolchain.toml`, the `Dockerfile` builder image and any CI image or toolchain reference (`.github` workflows) together, fix any new clippy/rustc lint fallout in that PR, and run `scripts/check-toolchain-pin.sh`.
- Read [Design principles](README.md#design-principles) — non-negotiable.
- Read [STYLEGUIDE.md](STYLEGUIDE.md) — binding rules.

## Adding a new module

Example: adding `foo` module. First pick namespace per [STYLEGUIDE.md → Module namespaces](STYLEGUIDE.md#module-namespaces).

### 1. Create module file

`crates/neoc-vm/src/lua/lib/foo.rs`:

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

### 2. Register

Add to `crates/neoc-vm/src/lua/lib/mod.rs` — `pub mod` declaration + `install` body. Alphabetical.

```rust
pub mod foo;
// ...
modules.insert("lib:foo".into(), foo::module(lua)?);
```

### 3. Rust tests

`#[cfg(test)] mod tests` at bottom of module file:

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

Full engine: `crate::lua::sandbox::make_engine()`. Run: `cargo test --workspace --lib`.

### 4. Criterion benches

`pub fn benches(c: &mut Criterion)` gated behind `bench` feature. Three tiers: `library` (pure Rust), `into_out_lua` (marshalling cost), `through_lua` (full user path).

```rust
#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use criterion::black_box;

    let lua = crate::lua::sandbox::make_engine().expect("engine");
    let payload = lua.create_string("world").unwrap();

    let mut g = c.benchmark_group("foo");

    g.bench_function("greet/library", |b| {
        b.iter(|| { let _ = black_box("hello, world"); });
    });

    let ident: mlua::Function = lua
        .create_function(|_, s: mlua::String| Ok(s))
        .unwrap();
    g.bench_function("greet/into_out_lua", |b| {
        b.iter(|| { let _: mlua::String = ident.call(black_box(payload.clone())).unwrap(); });
    });

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

Add to `criterion_group!` in `crates/neoc-vm/benches/lib/lib.rs`. Run: `cargo bench --bench lib foo`.

### 5. Lua integration test

`crates/neoc/tests/<ns>/foo.test.lua`:

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

Run: `cargo run -p neoc -- run crates/neoc/tests/<ns>/foo.test.lua`.

### 6. Update README

Add row to modules table. Alphabetical within namespace.

## Adding to existing module

1. Add function in `crates/neoc-vm/src/lua/lib/<name>.rs`
2. Register on table in `module()`
3. Add `#[cfg(test)]` cases
4. Extend bench group (3 tiers)
5. Add `describe(...)` cases to test file

## Pre-PR checks

```sh
cargo build
cargo test --workspace
cargo run -p neoc -- run 'crates/neoc/tests/**/*.test.lua'
cargo bench --bench lib <name>
cargo clippy --all-targets
cargo fmt --check
cargo deny check advisories
```

`cargo test --workspace` covers the `neoc-vm` unit tests, the `neoc` binary tests, the CLI integration tests and the Gherkin spec in `tooling/cli/`. The `**` Lua glob includes the Docker-dependent suites (`sqlx_mysql`, `sqlx_postgres`, `testcontainers_modules`); without a Docker daemon run the same set CI does:

```sh
cargo run -p neoc -- run \
  "crates/neoc/tests/lib/*.test.lua" \
  "crates/neoc/tests/std/*.test.lua" \
  "crates/neoc/tests/vnd/jsonschema.test.lua" \
  "crates/neoc/tests/vnd/quick_xml.test.lua" \
  "crates/neoc/tests/vnd/rusqlite.test.lua" \
  "crates/neoc/tests/vnd/serde_json.test.lua" \
  "crates/neoc/tests/vnd/serde_yaml.test.lua" \
  "crates/neoc/tests/vnd/toml.test.lua"
```

## CI

GitHub Actions (`.github/workflows/ci.yml`) on push to `main` and `develop` (docs, Markdown, media, CODEOWNERS and `.claude` changes excluded), on manual dispatch, and weekly. Never per PR (limited CI minutes): run the [pre-PR checks](#pre-pr-checks) locally. Dispatch a run on a branch with `gh workflow run ci.yml --ref <branch>`; follow it with `gh run watch <run-id> --exit-status` and read PR check status with `gh pr checks`.

Jobs: `lint` (`fmt --check`, `clippy` with warnings = errors), `deny`, `test` (Rust + Lua integration), `bench` (dispatch and schedule only), `bench:smoke`, the release-binary builds and `docker:build`. Release binaries are built by the `Release` workflow (see [How to release](#how-to-release)).

### Branching

- Implementation: `issues/{key}-{slug}` (e.g. `issues/NEOC-6-std-task`)
- Design: `design/{key}-{slug}`

### How to release

Releases are cut by pushing a tag. There is no release bot and no release PR.

```sh
git checkout main && git pull
git tag vX.Y.Z
git push origin vX.Y.Z
```

The tag must match `vMAJOR.MINOR.PATCH`, optionally with a `-<pre>` suffix
(`v0.2.0-rc.1`), and must point at a commit on `main`. The `Release` workflow
(`.github/workflows/release.yml`):

1. Stamps the tag's version into the workspace `Cargo.toml` in the CI checkout
   only (`scripts/stamp-version.sh`), so `neoc --version` reports it. Nothing is
   committed back.
2. Builds `neoc-x86_64-unknown-linux-gnu` and `neoc-x86_64-pc-windows-gnu.exe`.
3. Creates the GitHub Release with generated notes, both binaries, and
   `SHA256SUMS`. A `-<pre>` tag is marked as a pre-release.

Any build failure, or a tag not on `main`, fails the workflow without publishing.
It needs only the default `GITHUB_TOKEN`. `scripts/install.sh` and
`scripts/install.ps1` install from the latest non-pre-release; set
`NEOC_VERSION=vX.Y.Z-rc.1` to install a pre-release.

## Commit style

[Conventional Commits](https://www.conventionalcommits.org/).

```
<type>(<scope>): <subject>
```

| Type | When |
|---|---|
| `feat` | New capability |
| `fix` | Bug fix |
| `docs` | Documentation only |
| `refactor` | No behaviour change |
| `test` | Tests only |
| `chore` | Maintenance |
| `perf` | Performance |

Scope optional (module name or subsystem). Breaking: `!` suffix + `BREAKING CHANGE:` footer. Subject lowercase, no period. One logical change per commit.

## PR expectations

- One module or one well-scoped change per PR.
- Say *why* in description, not just *what*.
- Update docs in same PR.
- New crates require sign-off per [STYLEGUIDE.md](STYLEGUIDE.md#allowed-dependencies).
- Benchmark regressions: call out + explain in description.
- Reference the Jira key (`Refs NEOC-<n>`). Open against `develop`; mark ready for review when the pre-PR checks pass.

## When in doubt

- Module shape / errors / namespacing → [STYLEGUIDE.md](STYLEGUIDE.md)
- Design trade-offs → [README.md](README.md#design-principles)
- Testing / benching → this file + [README.md](README.md#module-workflow)
