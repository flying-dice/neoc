---
status: draft
created: 2026-05-05
implemented-in:
---

# 0007 — std:path — File path manipulation

> [!WARNING]
> This RFC is `draft`. Not the contract.

## Summary

`std:path` — two surfaces: Rust-mirror UserData types (`Path`, `PathBuf`) for type-safe chainable manipulation + flat convenience functions (Node/Deno naming) for common cases. OS-native separators preserved; no forward-slash normalisation. See [issue #19](https://github.com/flying-dice/neoc/issues/19).

## Motivation

Scripts manipulating paths without a dedicated module resort to `string.format` + manual separators → breaks cross-platform, silent misbehaviour on Windows backslashes.

Every major runtime ships a path module. neoc has none. Forces hard-coded platform assumptions or missing Lune shims.

Dual-surface rationale:

1. **Correctness/chainability** — `Path`/`PathBuf` UserData types make operations type-safe. Return values are unambiguously paths, not arbitrary strings.
2. **Ergonomics** — flat functions match Node names (`path.join`, `path.dirname`, `path.extname`). No secondary type import needed for simple ops.

Immutability distinction valuable even without borrow semantics: `Path` receiver → promise not to mutate; `PathBuf` → builder accumulating components. Confirmed per [issue #19](https://github.com/flying-dice/neoc/issues/19).

## Peer runtime survey

| Concept | Rust | Go | Node | Python | Deno | Bun | Lune |
|---|---|---|---|---|---|---|---|
| **Immutable path type** | `Path` (borrowed) | — | — | `PurePath` | — | — | — |
| **Mutable path type** | `PathBuf` (owned) | — | — | `Path` (mutable) | — | — | — |
| **Flat join** | `Path::join` | `filepath.Join` | `path.join` | `os.path.join` | `@std/path join` | `path.join` | — |
| **Directory component** | `Path::parent` | `filepath.Dir` | `path.dirname` | `os.path.dirname` | `dirname` | `path.dirname` | — |
| **File name** | `Path::file_name` | `filepath.Base` | `path.basename` | `os.path.basename` | `basename` | `path.basename` | — |
| **Extension** | `Path::extension` | `filepath.Ext` | `path.extname` | `os.path.splitext` | `extname` | `path.extname` | — |
| **Normalize** | `Path::components` (lexical) | `filepath.Clean` | `path.normalize` | `os.path.normpath` | `normalize` | `path.normalize` | — |
| **Absolute check** | `Path::is_absolute` | `filepath.IsAbs` | `path.isAbsolute` | `os.path.isabs` | `isAbsolute` | `path.isAbsolute` | — |
| **Resolve to absolute** | `Path::canonicalize`† | `filepath.Abs` | `path.resolve` | `os.path.abspath` | `resolve` | `path.resolve` | — |
| **Relative path** | `Path::strip_prefix` | `filepath.Rel` | `path.relative` | `os.path.relpath` | `relative` | `path.relative` | — |
| **OS separator** | `std::path::MAIN_SEPARATOR` | `string(os.PathSeparator)` | `path.sep` | `os.sep` | `SEPARATOR` | `path.sep` | — |
| **OOP chaining** | `PathBuf::push`, `:join` | — | — | `Path / "child"` | — | — | — |

†`canonicalize` requires filesystem access. Deferred to `std:fs`; see design decisions.

**Observations:**

1. `join`, `dirname`, `basename`, `extname` — universal. Non-negotiable core.
2. Only Rust provides immutable + mutable types. Python's `pathlib.Path` is mutable. Node/Deno/Go use strings only.
3. Extension format diverges: Rust `"txt"` (no dot) vs Node `".txt"` (with dot). Both preserved on respective surfaces — intentional documented difference.
4. Lune ships no path module. Authors use raw strings. Clear gap.
5. Node/Deno `path.resolve` is CWD-dependent + filesystem access. neoc's `std:path.resolve` prepends `std:env.current_dir()` + lexical normalize — no syscall. `std:path` is lexical-only.

## Detailed design

### Module registration

```
src/lua/std/path.rs             -- module implementation (Path, PathBuf UserData + flat functions)
src/lua/std/mod.rs              -- registration (alphabetical insert)
tests/std/path.test.lua        -- Lua integration tests
(type definition stub for std:path)
```

### Lua surface

```lua
local path = require("std:path")
```

### UserData types — `Path` and `PathBuf`

#### `Path` — immutable

Wraps a string; read-only inspection. Methods returning paths → new `PathBuf`. Component methods → strings or `nil`.

```lua
local p = path.Path.new("/home/user/file.txt")

-- Component inspection
p:parent()              --> Path("/home/user") or nil if no parent
p:file_name()           --> "file.txt" or nil (last component)
p:file_stem()           --> "file" or nil (file name without extension)
p:extension()           --> "txt" or nil (no leading dot — Rust convention)
p:components()          --> { "/", "home", "user", "file.txt" } (array of strings)

-- Predicate methods
p:is_absolute()         --> true
p:is_relative()         --> false
p:starts_with("/home")  --> true
p:ends_with("file.txt") --> true

-- Transformations — return PathBuf
-- Note: :join appends a path component blindly. The caller is responsible for
-- ensuring the receiver is a directory-typed path. Using a file path as the
-- receiver produces a semantically invalid path (a component appended beneath
-- what is actually a file). Use a directory-valued receiver:
local dir = path.Path.new("/home/user")
dir:join("sub")                 --> PathBuf("/home/user/sub")
p:with_extension("rs")          --> PathBuf("/home/user/file.rs")
p:with_file_name("other.rs")    --> PathBuf("/home/user/other.rs")
p:strip_prefix("/home")         --> Path("user/file.txt") — errors if not a prefix

-- Display
p:display()     --> "/home/user/file.txt" (OS-native separators)
tostring(p)     --> "/home/user/file.txt" (via __tostring metamethod)
```

#### `PathBuf` — mutable

Extends `Path` with in-place mutation. All `Path` read methods available (Deref equivalent via UserData implementation).

```lua
local buf = path.PathBuf.new()         -- empty mutable path
local buf = path.PathBuf.from("/home/user")  -- from string

-- Mutation (in-place)
buf:push("docs")            -- append component; no return value
buf:pop()                   --> bool — removes last component; returns false if already empty
buf:set_extension("md")     -- replace or append extension; no return value
buf:set_file_name("readme.md")  -- replace last component; no return value

-- All Path read methods available:
buf:parent()
buf:file_name()
buf:extension()
buf:components()
buf:is_absolute()
buf:join("child")           --> PathBuf (new)
buf:with_extension("rs")    --> PathBuf (new)
tostring(buf)               --> string
```

#### Equality and comparison

Both types implement `__eq`. Equal if string representations identical after trailing-separator normalisation (follows Rust `PartialEq` for `Path`).

```lua
path.Path.new("/home/user") == path.Path.new("/home/user")  --> true
```

Case-sensitive on all platforms including Windows — matches Rust, avoids platform-specific case folding cost. For case-insensitive: `:display()` + `string.lower`.

Cross-type equality (`Path == PathBuf`) — **open question #5**. Do not depend on it until resolved.

### Flat convenience functions

Operate on plain strings, return plain strings. Stateless, no filesystem access. Node/Deno naming; Rust/POSIX semantics where traditions diverge.

```lua
path.join("src", "lua", "lib", "json.rs")      --> "src/lua/lib/json.rs"
path.dirname("/home/user/file.txt")             --> "/home/user"
path.basename("/home/user/file.txt")            --> "file.txt"
path.extname("/home/user/file.txt")             --> ".txt" (with leading dot — Node convention)
path.normalize("src/../src/./lua")              --> "src/lua"
path.is_absolute("/home/user")                  --> true
-- given CWD = "/home/user":
path.resolve("relative/path")                  --> "/home/user/relative/path"
path.relative("/home/user", "/home/other")      --> "../other"
path.sep                                        --> OS-native separator string ("/" or "\\")
```

#### Function semantics

- **`path.join(...)`** — variadic, OS-native separator. Absolute argument resets result (Rust/POSIX — differs from Node which treats all segments as relative). Zero args → `""`; single empty string → `""`.
- **`path.dirname(p)`** — directory component. Bare filename → `"."`. Root → unchanged (`dirname("/") == "/"`; matches Node/Python). `dirname("") == "."`. Note: Rust `Path::new("/").parent()` returns `None`; implementation must special-case root rather than falling back to `"."`.
- **`path.basename(p)`** — last component; trailing separators ignored. Root (`"/"`, `"C:\\"`) where `file_name()` → `None`: returns `"/"` or `"C:\\"` (matches Node). `basename("") == ""`.
- **`path.extname(p)`** — extension with leading dot (`.txt`), or `""` if none. Dotfiles (`.gitignore`, `.env`) → `""` (entire name = stem, no extension). Matches Node and Rust. `extname("") == ""`.
- **`path.normalize(p)`** — lexical: resolves `.`, collapses `..`, removes duplicate separators. No filesystem access. `..` past root clamped: `normalize("/../foo") == "/foo"` (Go/Node/Python). `normalize("") == "."`.
- **`path.is_absolute(p)`** — `true` if absolute.
- **`path.resolve(p)`** — prepends `std:env.current_dir()` if relative, then `normalize`. No filesystem access beyond CWD query. Already-absolute → normalised unchanged.
- **`path.relative(from, to)`** — lexical relative path. Both args treated as directories. Equivalent to Go `filepath.Rel` / Node `path.relative`.
- **`path.sep`** — `"/"` on Unix, `"\\"` on Windows.

### Key differences between surfaces

| Concern | UserData (`Path`/`PathBuf`) | Flat functions |
|---|---|---|
| Extension format | `"txt"` (no dot — Rust) | `".txt"` (with dot — Node) |
| Return types | `Path`/`PathBuf` UserData or strings | Plain strings |
| Mutation | `PathBuf` mutates in-place | Stateless, new strings |
| Chaining | `p:join("a"):join("b"):with_extension("rs")` | Nest calls or intermediates |
| Null-safety | `:parent()` → `nil` on root | `dirname("/")` → `"/"` |

### Error semantics

Format: `"module.function: <detail>"`.

```
"path.strip_prefix: prefix not found"
"path.resolve: current_dir unavailable: <reason>"
"path: non-UTF-8 path component"
```

| Function | Condition | Behaviour |
|---|---|---|
| `Path:strip_prefix(prefix)` | Prefix mismatch | `error("path.strip_prefix: prefix not found")` — programmer error |
| `path.resolve(p)` | `current_dir()` fails | `error("path.resolve: current_dir unavailable: <reason>")` |
| Any function | Non-UTF-8 bytes | `error("path: non-UTF-8 path component")` |
| `PathBuf:push("")` | Empty string | No-op (matches Rust) |
| `path.Path.new("")` | Empty string | Valid empty path (matches Rust) |
| `path.PathBuf.new()` | Zero args | Valid empty mutable path (matches Rust) |

`strip_prefix` errors via `error()` — programmer error, not recoverable (mirrors Rust `Err`). All other ops infallible for valid UTF-8.

Non-UTF-8 paths: neoc targets UTF-8 throughout. Non-UTF-8 bytes → `error("path: non-UTF-8 path component")`.

### Rust implementation sketch

```rust
// src/lua/std/path.rs
use mlua::{Lua, MetaMethod, Result, Table, UserData, UserDataMethods};
use std::path::{Path as StdPath, PathBuf as StdPathBuf};

#[derive(Clone)]
pub struct LuaPath(StdPathBuf);

#[derive(Clone)]
pub struct LuaPathBuf(StdPathBuf);

impl UserData for LuaPath {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("parent", |_, this, ()| {
            Ok(this.0.parent().map(|p| LuaPath(p.to_path_buf())))
        });
        methods.add_method("file_name", |_, this, ()| {
            Ok(this.0.file_name().and_then(|s| s.to_str()).map(String::from))
        });
        methods.add_method("file_stem", |_, this, ()| {
            Ok(this.0.file_stem().and_then(|s| s.to_str()).map(String::from))
        });
        methods.add_method("extension", |_, this, ()| {
            Ok(this.0.extension().and_then(|s| s.to_str()).map(String::from))
        });
        methods.add_method("is_absolute", |_, this, ()| Ok(this.0.is_absolute()));
        methods.add_method("is_relative", |_, this, ()| Ok(this.0.is_relative()));
        methods.add_method("join", |_, this, other: String| {
            Ok(LuaPathBuf(this.0.join(other)))
        });
        methods.add_method("starts_with", |_, this, base: String| {
            Ok(this.0.starts_with(base))
        });
        methods.add_method("ends_with", |_, this, child: String| {
            Ok(this.0.ends_with(child))
        });
        methods.add_method("strip_prefix", |_, this, prefix: String| {
            this.0.strip_prefix(&prefix)
                .map(|p| LuaPath(p.to_path_buf()))
                .map_err(|_| mlua::Error::RuntimeError(
                    "path.strip_prefix: prefix not found".to_string()
                ))
        });
        methods.add_method("components", |lua, this, ()| {
            let tbl = lua.create_table()?;
            for (i, c) in this.0.components().enumerate() {
                tbl.set(i + 1, c.as_os_str().to_string_lossy().as_ref())?;
            }
            Ok(tbl)
        });
        methods.add_method("with_extension", |_, this, ext: String| {
            Ok(LuaPathBuf(this.0.with_extension(ext)))
        });
        methods.add_method("with_file_name", |_, this, name: String| {
            Ok(LuaPathBuf(this.0.with_file_name(name)))
        });
        methods.add_method("display", |_, this, ()| {
            Ok(this.0.to_string_lossy().into_owned())
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(this.0.to_string_lossy().into_owned())
        });
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: LuaPath| {
            Ok(this.0 == other.0)
        });
    }
}

impl UserData for LuaPathBuf {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // Mutation methods
        methods.add_method_mut("push", |_, this, component: String| {
            this.0.push(component);
            Ok(())
        });
        methods.add_method_mut("pop", |_, this, ()| Ok(this.0.pop()));
        methods.add_method_mut("set_extension", |_, this, ext: String| {
            this.0.set_extension(ext);
            Ok(())
        });
        methods.add_method_mut("set_file_name", |_, this, name: String| {
            this.0.set_file_name(name);
            Ok(())
        });
        // Delegate all Path read methods via shared helper or duplication
        // (Rust lacks Deref coercion in mlua; methods are registered identically)
        methods.add_method("parent", |_, this, ()| {
            Ok(this.0.parent().map(|p| LuaPath(p.to_path_buf())))
        });
        // ... remaining Path read methods elided for brevity — same as LuaPath
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(this.0.to_string_lossy().into_owned())
        });
    }
}

pub fn module(lua: &Lua) -> Result<Table> {
    let t = lua.create_table()?;

    // UserData constructors
    let path_tbl = lua.create_table()?;
    path_tbl.set("new", lua.create_function(|_, s: String| {
        Ok(LuaPath(StdPathBuf::from(s)))
    })?)?;
    t.set("Path", path_tbl)?;

    let pathbuf_tbl = lua.create_table()?;
    pathbuf_tbl.set("new", lua.create_function(|_, ()| {
        Ok(LuaPathBuf(StdPathBuf::new()))
    })?)?;
    pathbuf_tbl.set("from", lua.create_function(|_, s: String| {
        Ok(LuaPathBuf(StdPathBuf::from(s)))
    })?)?;
    t.set("PathBuf", pathbuf_tbl)?;

    // Flat convenience functions
    t.set("join", lua.create_function(|_, args: mlua::Variadic<String>| {
        let mut buf = StdPathBuf::new();
        for seg in args.iter() {
            buf.push(seg);
        }
        Ok(buf.to_string_lossy().into_owned())
    })?)?;

    t.set("dirname", lua.create_function(|_, s: String| {
        let p = StdPath::new(&s);
        Ok(match p.parent() {
            Some(d) if d.as_os_str().is_empty() => ".".to_string(),  // bare filename: "file.txt" -> "."
            Some(d) => d.to_string_lossy().into_owned(),              // normal case: "/a/b" -> "/a"
            None => s,  // root path ("/", "C:\\") has no parent — return as-is
        })
    })?)?;

    t.set("basename", lua.create_function(|_, s: String| {
        let p = StdPath::new(&s);
        Ok(p.file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| s.clone()))
    })?)?;

    t.set("extname", lua.create_function(|_, s: String| {
        let p = StdPath::new(&s);
        Ok(p.extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default())
    })?)?;

    t.set("normalize", lua.create_function(|_, s: String| {
        // Lexical normalization: resolve . and collapse .. where possible.
        // Uses path-clean crate logic or manual implementation — no syscall.
        Ok(lexical_normalize(&s))
    })?)?;

    t.set("is_absolute", lua.create_function(|_, s: String| {
        Ok(StdPath::new(&s).is_absolute())
    })?)?;

    t.set("resolve", lua.create_function(|_, s: String| {
        if StdPath::new(&s).is_absolute() {
            return Ok(lexical_normalize(&s));
        }
        let cwd = std::env::current_dir()
            .map_err(|e| mlua::Error::RuntimeError(format!("path.resolve: current_dir unavailable: {e}")))?;
        Ok(lexical_normalize(&cwd.join(&s).to_string_lossy()))
    })?)?;

    t.set("relative", lua.create_function(|_, (from, to): (String, String)| {
        // Pure lexical relative path computation — no filesystem access.
        Ok(lexical_relative(&from, &to))
    })?)?;

    t.set("sep", std::path::MAIN_SEPARATOR.to_string())?;

    Ok(t)
}

fn lexical_normalize(path: &str) -> String {
    // Implement . removal and .. collapse lexically.
    // Equivalent to Go's filepath.Clean / Node's path.normalize.
    todo!("lexical normalize — see implementation notes")
}

fn lexical_relative(from: &str, to: &str) -> String {
    // Compute relative path from `from` to `to` without filesystem access.
    todo!("lexical relative — see implementation notes")
}
```

### Type definition stub

```lua
-- std:path type stub

export type Path = {
    parent:         (self: Path) -> Path?,
    file_name:      (self: Path) -> string?,
    file_stem:      (self: Path) -> string?,
    extension:      (self: Path) -> string?,
    components:     (self: Path) -> { string },
    is_absolute:    (self: Path) -> boolean,
    is_relative:    (self: Path) -> boolean,
    join:           (self: Path, other: string) -> PathBuf,
    starts_with:    (self: Path, base: string) -> boolean,
    ends_with:      (self: Path, child: string) -> boolean,
    strip_prefix:   (self: Path, prefix: string) -> Path,  -- errors if not a prefix
    with_extension: (self: Path, ext: string) -> PathBuf,
    with_file_name: (self: Path, name: string) -> PathBuf,
    display:        (self: Path) -> string,
}

export type PathBuf = Path & {
    push:          (self: PathBuf, component: string) -> (),
    pop:           (self: PathBuf) -> boolean,
    set_extension: (self: PathBuf, ext: string) -> (),
    set_file_name: (self: PathBuf, name: string) -> (),
}

declare path: {
    Path: {
        new: (s: string) -> Path,
    },
    PathBuf: {
        new: () -> PathBuf,
        from: (s: string) -> PathBuf,
    },

    join:        (...string) -> string,
    dirname:     (p: string) -> string,
    basename:    (p: string) -> string,
    extname:     (p: string) -> string,
    normalize:   (p: string) -> string,
    is_absolute: (p: string) -> boolean,
    resolve:     (p: string) -> string,
    relative:    (from: string, to: string) -> string,
    sep:         string,
}
```

## Security considerations

`std:path` performs no filesystem access except `path.resolve` (reads CWD via `std::env::current_dir()`). No file opened/read/written/stat-ed. Safe without filesystem capability.

1. **Path traversal.** `Path:join()`/`path.join()` consumers must validate results stay within intended root before passing to `std:fs`. Module provides `Path:starts_with()` and `Path:strip_prefix()` for that. Lexical ops cannot resolve symlinks → cannot enforce security boundaries involving symlinks. Use `std:fs.canonicalize()` (future) when symlink resolution required.
2. **Capability gating.** `std:path` (lexical-only) needs no `--allow-fs`/`--allow-path`. `path.resolve` uses `current_dir()` → inherits `std:env` capability scope if gated.

## Drawbacks

- **Dual surface doubles docs.** Two conventions (extension with/without dot, UserData vs string) require clear documentation. Mitigated by explicit table + consistent naming (`Path:extension` vs `path.extname`).
- **No Deref coercion in mlua.** `PathBuf` inherits `Path` read methods by duplication. Maintenance burden mitigated by shared `register_path_read_methods` helper.
- **Lexical-only normalize cannot resolve symlinks.** Authors needing symlink-aware normalisation → `std:fs.canonicalize()`. Documented.
- **Case-sensitive comparison on all platforms.** Windows paths case-insensitive in practice, but `Path.__eq` is case-sensitive (matches Rust). Consistent with Rust-mirror precedent; avoids platform-specific complexity.

## Alternatives

### Strings only (Node/Go approach)

Rejected. Rust-mirror runtime should expose Rust's type model. `Path`/`PathBuf` provides type-level guarantees strings cannot. Flat functions = ergonomic sugar, not primary surface.

### Single mutable type (Python `pathlib.Path`)

Rejected. Immutability distinction confirmed useful without borrow semantics. `Path` receiver signals no mutation; `PathBuf` may. Expressible in Luau types, meaningful at call site.

### Single flat surface, no UserData

Rejected. Cannot chain without intermediates. `p:join("a"):join("b"):with_extension("rs")` beats nested calls. Worth the complexity.

### Include `canonicalize` in `std:path`

Rejected. Requires filesystem access (symlinks, directory existence). `std:path` deliberately lexical-only. Belongs in `std:fs`.

### `path.sep` as function

Rejected. Compile-time constant. Function implies runtime variability. Every other runtime exposes as constant.

### Lune approach (no path module)

Rejected. Known gap forcing manual separator manipulation. neoc targets professional-grade scripting; cross-platform correctness not optional.

## Open questions

1. **`lexical_normalize` implementation.** No Rust stdlib function does lexical normalize without filesystem (only `canonicalize`). Use `path-clean` crate or in-tree (~50 lines). Decision needed before implementation PR.

2. **`path.join` variadic signature.** Flat `path.join` → variadic (Node). `PathBuf:push` → single arg (Rust). Asymmetry intentional; confirm before landing.

3. **`path.relative` cross-drive edge case.** Different Windows drive letters: Go errors; Node returns absolute `to`. Recommendation: Node behaviour (always valid).

4. **Cross-type method calls.** Should `Path:join` accept `PathBuf` arg? Currently string-only. Accepting `PathBuf` requires `AnyUserData` + downcast. Deferred to implementation.

5. **`PathBuf` equality with `Path`.** mlua `__eq` only fires for same UserData type. Cross-type may need workaround (e.g. `__tostring` fallback). Unspecified until resolved — do not depend on it.

## Implementation notes

- **Files:** `src/lua/std/path.rs`, registered in `src/lua/std/mod.rs` alphabetically.
- **Shared helper:** `fn register_path_read_methods<T: UserData + AsRef<StdPath>>(methods: &mut UserDataMethods<T>)` — avoid duplicating `Path` methods on `PathBuf`.
- **Lexical normalize:** `path-clean = "1"` or in-tree. Document in PR.
- **`path.resolve` CWD:** calls `std::env::current_dir()`. Failure → `error("path.resolve: current_dir unavailable: <reason>")`.
- **Tests:** `tests/std/path.test.lua` — all acceptance criteria. Separator-sensitive assertions conditional on `require("std:env").os`. Cover: `path.basename("/")`, `path.extname(".gitignore")`, `path.normalize("/../foo")`, all functions with `""`.
- **Type stub:** `std:path` type definition — required per M1 DoD. `PathBuf = Path & { ... }` models inheritance.
- **Bench:** Criterion, `bench` feature. Minimum: `join` (10 segments), `normalize` (multiple `..`).
- **Sequencing:** No dependency on `std:fs` or `std:env` at module level. `path.resolve` calls Rust stdlib `current_dir()` directly. Lands independently; no blockers.
