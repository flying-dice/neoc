# Contributing

Mechanical walkthrough — order, file locations, pre-MR checks. Points to [STYLEGUIDE.md](STYLEGUIDE.md) and [README.md](README.md) for conventions and rationale.

## Before you start

- `rustup` + `cargo` on `PATH`. Toolchain pinned in [rust-toolchain.toml](rust-toolchain.toml) — rustup auto-installs it; don't use `+stable`.
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

Add to `criterion_group!` in `crates/neoc/benches/lib/lib.rs`. Run: `cargo bench --bench lib foo`.

### 5. Luau integration test

`crates/neoc/tests/<ns>/foo.test.luau`:

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

Run: `cargo run -p neoc -- run crates/neoc/tests/<ns>/foo.test.luau`.

### 6. Update README

Add row to modules table. Alphabetical within namespace.

## Adding to existing module

1. Add function in `crates/neoc-vm/src/lua/lib/<name>.rs`
2. Register on table in `module()`
3. Add `#[cfg(test)]` cases
4. Extend bench group (3 tiers)
5. Add `describe(...)` cases to test file

## Pre-MR checks

```sh
cargo build
cargo test --workspace --lib
cargo test -p neoc --bin neoc
cargo run -p neoc -- run 'crates/neoc/tests/**/*.test.luau'
cargo bench --bench lib <name>
cargo clippy --all-targets
cargo fmt --check
```

## CI

GitLab CI (`.gitlab-ci.yml`) on push to `main` and MRs targeting `main`.

Stages:
1. **Lint** — `fmt --check`, `clippy` (warnings = errors)
2. **Test** — Rust + Luau integration (Docker tests excluded)
3. **Build** — cross-compiled binaries as artifacts
4. **Pages** — VitePress docs (main only)

### Cross-compilation

| Job | Target |
|---|---|
| `build:linux-x86_64` | `x86_64-unknown-linux-gnu` |
| `build:windows-x86_64` | `x86_64-pc-windows-gnu` |

### Branching

- Implementation: `issues/{id}-{slug}`
- Design: `design/{id}-{slug}`

### Releasing

[release-plz](https://release-plz.dev/) via conventional commits:
1. Merge to `main` with conventional commit messages
2. release-plz creates/updates Release MR (version bump + changelog)
3. Merge Release MR = approval gate
4. Tag pushed → GitLab Release created
5. Build pipeline: lint → test → cross-compile → upload binaries

Requires `GITLAB_TOKEN` CI variable (`api` + `write_repository` scopes).

## Commit style

[Conventional Commits](https://www.conventionalcommits.org/). release-plz reads these.

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

## MR expectations

- One module or one well-scoped change per MR.
- Say *why* in description, not just *what*.
- Update docs in same MR.
- New crates require sign-off per [STYLEGUIDE.md](STYLEGUIDE.md#allowed-dependencies).
- Benchmark regressions: call out + explain in description.
- Apply `workflow::Implementation` label, update to `workflow::Review` when ready.

## When in doubt

- Module shape / errors / namespacing → [STYLEGUIDE.md](STYLEGUIDE.md)
- Design trade-offs → [README.md](README.md#design-principles)
- Testing / benching → this file + [README.md](README.md#module-workflow)
