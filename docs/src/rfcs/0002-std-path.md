---
status: draft
created: 2026-05-05
implemented-in:
---

# 0002 — std:path — File path manipulation

> [!WARNING]
> This RFC is `draft`. It is under refinement and is **not** the contract.

## Summary

Add `std:path` as a first-class module in the neoc standard library. The module exposes two complementary surfaces: Rust-mirror UserData types (`Path` and `PathBuf`) for type-safe, chainable path manipulation, and a set of flat convenience functions following Node/Deno naming conventions for ergonomic common-case use. OS-native separators are preserved throughout; no forward-slash normalisation is performed. See [issue #19](https://github.com/flying-dice/neoc/issues/19) for the full design rationale.

## Motivation

Scripts that touch the filesystem — reading configs, constructing output paths, resolving relative references — inevitably manipulate path strings. Without a dedicated module, authors reach for `string.format` and manual separator handling, which breaks cross-platform and silently misbehaves on Windows paths containing backslashes.

Every major runtime ships a path module. neoc has none. The absence forces authors to either hard-code platform assumptions or depend on Lune compatibility shims that neoc does not expose. This module closes that gap.

Two distinct design goals motivate the dual-surface approach:

1. **Correctness and chainability** — the `Path`/`PathBuf` UserData types make path operations type-safe. A function that returns a `Path` can have `:join()` called on it directly; there is no ambiguity about whether a return value is a path or an arbitrary string.
2. **Ergonomics for common cases** — the flat convenience functions match the names authors already know from Node (`path.join`, `path.dirname`, `path.extname`). No import of a secondary type is needed for simple operations.

The immutability distinction between `Path` and `PathBuf` is explicitly valuable even without Rust's borrow semantics. A function that promises not to mutate its argument can accept a `Path`; a builder accumulating path components takes a `PathBuf`. This distinction is confirmed design direction per [issue #19](https://github.com/flying-dice/neoc/issues/19).

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

†`canonicalize` requires filesystem access. Deferred to `std:fs` integration; see design decisions.

**Key observations:**

1. Every major runtime provides `join`, `dirname`/`dir`, `basename`/`base`, and `extname`/`ext`. These are the non-negotiable core.
2. Rust is the only runtime that provides both an immutable borrowed type (`Path`) and an owned mutable type (`PathBuf`). Python's `pathlib.Path` is mutable. Node, Deno, and Go work exclusively with strings.
3. Extension format differs between the two traditions: Rust's `Path::extension()` returns `"txt"` (no leading dot); Node/Deno's `path.extname()` returns `".txt"` (with leading dot). Both conventions are preserved on their respective surfaces — this is an intentional, documented difference.
4. Lune (the closest Luau precedent) ships no path module at all. Authors using Lune manipulate paths as raw strings. This is a clear gap this module fills.
5. `path.resolve` in Node/Deno is CWD-dependent and performs filesystem access. In a sandbox, this requires explicit capability. `std:path.resolve` in neoc returns an absolute path by prepending `std:env.current_dir()` and applying lexical normalization — no filesystem syscall, consistent with the principle that `std:path` is lexical-only.

## Detailed design

### Module registration

```
src/lua/std/path.rs             -- module implementation (Path, PathBuf UserData + flat functions)
src/lua/std/mod.rs              -- registration (alphabetical insert)
tests/std/path.test.luau        -- Luau integration tests
types/std/path.d.luau           -- type definition stubs
```

### Lua surface

```lua
local path = require("std:path")
```

### UserData types — `Path` and `PathBuf`

#### `Path` — immutable

`Path` wraps a string and exposes read-only path inspection. All methods that return a path return a new `PathBuf` (mutable), so the result can be further modified. Methods that return components return plain strings or `nil`.

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

`PathBuf` extends `Path` with in-place mutation. All `Path` read methods are available on `PathBuf` (Deref equivalent via the UserData implementation).

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

Both types implement `__eq`. Two paths are equal if their string representations are identical after normalisation of trailing separators (following Rust's `PartialEq` for `Path`).

```lua
path.Path.new("/home/user") == path.Path.new("/home/user")  --> true
```

#### Equality notes

Comparison is case-sensitive on all platforms, including Windows — this matches Rust's behaviour and avoids the cost of platform-specific case folding in the Lua layer. Authors who need case-insensitive comparison should call `:display()` and apply `string.lower`.

Cross-type equality (`Path == PathBuf`) is an **open question** — see Open Question #5. The implementation may or may not support it depending on the resolution of that question. Do not write code that depends on cross-type equality until the open question is resolved.

### Flat convenience functions

All flat functions operate on plain strings and return plain strings. They are stateless and perform no filesystem access. Function naming follows Node/Deno conventions; semantics follow Rust/POSIX where the two traditions diverge (see individual function notes below).

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

- **`path.join(...)`** — joins any number of string segments using the OS-native separator. Does not call into the filesystem. If an argument is an absolute path, it resets the result (Rust/POSIX semantics — note: this differs from Node's `path.join`, which treats all segments as relative). Zero-argument call returns `""` (empty string); a single empty-string argument returns `""`.
- **`path.dirname(p)`** — returns the directory component. Returns `"."` for a bare filename with no directory component (e.g. `path.dirname("file.txt") == "."`). Returns the path unchanged for root paths (e.g. `path.dirname("/") == "/"`) — matching Node and Python behaviour. `path.dirname("") == "."` (matches Node). Note: `std::path::Path::new("/").parent()` returns `None` in Rust; the implementation must special-case root paths rather than falling back to `"."` for the `None` case.
- **`path.basename(p)`** — returns the last component of the path. Trailing separators are ignored. On root paths (`"/"`, `"C:\\"`) where `file_name()` returns `None`, returns `"/"` or `"C:\\"` respectively — matching Node (`path.basename("/") == "/"`). `path.basename("") == ""` (matches Node).
- **`path.extname(p)`** — returns the extension including the leading dot (`.txt`), or `""` if there is no extension. This intentionally differs from `Path:extension()`, which omits the dot. For dotfiles (e.g. `.gitignore`, `.env`), returns `""` — the entire name is treated as the stem with no extension. This is consistent with both Node (`path.extname(".gitignore") == ""`) and Rust (`Path::extension()` returns `None` for dotfiles — a name beginning with `.` and containing no subsequent `.` has no extension). `path.extname("") == ""`.
- **`path.normalize(p)`** — performs lexical normalisation: resolves `.` components, collapses `..` where possible, removes duplicate separators. Does not access the filesystem. Equivalent to Go's `filepath.Clean`. `..` past root is clamped: `path.normalize("/../foo") == "/foo"` (matching Go, Node, and Python). `path.normalize("") == "."` (matches Node).
- **`path.is_absolute(p)`** — returns `true` if the path is absolute.
- **`path.resolve(p)`** — returns an absolute path by prepending `std:env.current_dir()` if `p` is relative, then applying `path.normalize`. Does not access the filesystem beyond the CWD query. Example: given CWD `/home/user`, `path.resolve("docs/file.txt")` returns `"/home/user/docs/file.txt"`. Already-absolute paths are normalised and returned unchanged.
- **`path.relative(from, to)`** — computes the relative path from `from` to `to`. Both arguments are treated as directory paths. Equivalent to Go's `filepath.Rel` / Node's `path.relative`.
- **`path.sep`** — read-only string constant: `"/"` on Unix, `"\\"` on Windows.

### Key differences between the two surfaces

| Concern | UserData (`Path`/`PathBuf`) | Flat functions |
|---|---|---|
| Extension format | `"txt"` (no dot — Rust convention) | `".txt"` (with dot — Node convention) |
| Return types | `Path`/`PathBuf` UserData or strings | Plain strings |
| Mutation | `PathBuf` methods mutate in-place | Stateless, return new strings |
| Chaining | `p:join("a"):join("b"):with_extension("rs")` | Nest calls or intermediate variables |
| Null-safety | `:parent()` returns `nil` on root | `dirname("/")` returns `"/"` |

### Error semantics

Errors follow the `"module.function: <detail>"` convention:

```
"path.strip_prefix: prefix not found"
"path.resolve: current_dir unavailable: <reason>"
"path: non-UTF-8 path component"
```

The table below specifies error behaviour per function. Functions not listed are infallible for any string input (including empty string).

| Function | Condition | Behaviour |
|---|---|---|
| `Path:strip_prefix(prefix)` | Prefix does not match | `error("path.strip_prefix: prefix not found")` — programmer error, not recoverable |
| `path.resolve(p)` | `current_dir()` syscall fails | `error("path.resolve: current_dir unavailable: <reason>")` |
| Any function | Non-UTF-8 bytes in path | `error("path: non-UTF-8 path component")` |
| `PathBuf:push("")` | Empty string component | Silently appends nothing — Rust's `PathBuf::push("")` is a no-op |
| `path.Path.new("")` | Empty string | Constructs a valid empty path — equivalent to Rust's `Path::new("")` |
| `path.PathBuf.new()` | (zero args) | Constructs a valid empty mutable path — equivalent to Rust's `PathBuf::new()` |

`strip_prefix` errors via `error()` — this is a programmer error, not a recoverable condition, mirroring Rust's `Path::strip_prefix` which returns `Err`. All other path operations are infallible for valid UTF-8 input, including empty strings.

Non-UTF-8 paths: neoc targets UTF-8 throughout. Paths containing non-UTF-8 bytes are reported as an error with the prefix `"path: non-UTF-8 path component"`.

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
-- types/std/path.d.luau

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

`std:path` performs no filesystem access except in `path.resolve` (which reads the CWD via `std::env::current_dir()`). No file is opened, read, written, or stat-ed. The module is safe to grant without filesystem capability.

Two points worth documenting:

1. **Path traversal.** Consumers of `Path:join()` or `path.join()` must validate that the resulting path stays within an intended root before passing it to `std:fs`. This module provides `Path:starts_with()` and `Path:strip_prefix()` specifically for that validation. The absence of canonicalization in `std:path` is intentional: lexical operations cannot resolve symlinks, so they cannot be relied upon for security boundaries that involve symlinks. Use `std:fs.canonicalize()` (a future API) when symlink resolution is required.
2. **Capability gating.** If neoc adopts a `--allow-fs` or `--allow-path` capability model, `std:path` in its current scope (lexical only) does not require it. `path.resolve` uses `current_dir()` which is part of `std:env`'s capability scope. If `std:env` becomes capability-gated, `path.resolve` inherits that gate.

## Drawbacks

- **Dual surface doubles the documentation surface.** Two distinct conventions for the same operations (extension with/without dot, UserData vs. string returns) require clear documentation to avoid confusion. Mitigated by the explicit table in the design section and consistent naming (`Path:extension` vs `path.extname`).
- **UserData methods cannot use Rust's Deref coercion.** `PathBuf` inherits `Path`'s read methods by duplication in the UserData implementation, not through Rust's `Deref<Target = Path>`. This is a maintenance burden — any change to a `Path` method must be mirrored on `PathBuf`. Mitigated by extracting a shared `register_path_read_methods` helper.
- **Lexical-only `path.normalize` cannot resolve `..` past unknown symlinks.** Authors who need symlink-aware normalization must use `std:fs.canonicalize()`. This is documented.
- **Case-sensitive comparison on all platforms.** Windows paths are case-insensitive in practice, but neoc's `Path.__eq` is case-sensitive, matching Rust's `PartialEq for Path`. Authors on Windows may observe surprising equality failures. This is consistent with the Rust mirror precedent and avoids platform-specific complexity in the Lua layer.

## Alternatives

### Strings only (Node/Go approach)

Rejected. `std:path` in a Rust-mirror runtime should expose Rust's type model. The `Path`/`PathBuf` distinction provides type-level guarantees that string-only approaches cannot. The flat functions are included as ergonomic sugar, not as the primary surface.

### Single mutable type only (Python's `pathlib.Path`)

Rejected. The immutability distinction is explicitly confirmed by Jonathan as useful even without borrow semantics. A function that takes a `Path` signals it will not modify the path; a function that takes a `PathBuf` may. This is expressible in Luau type definitions and meaningful at the call site.

### Single flat surface with no UserData types

Rejected. Flat string functions cannot be chained without intermediate variables. `p:join("a"):join("b"):with_extension("rs")` is more readable than `path.with_extension(path.join(path.join(p, "a"), "b"), "rs")`. The UserData surface is worth the complexity.

### Include `canonicalize` in `std:path`

Rejected. `canonicalize` requires filesystem access — it must resolve symlinks and verify that intermediate directories exist. `std:path` is deliberately lexical-only. `canonicalize` belongs in `std:fs`, where filesystem capability is already implied.

### Expose `path.sep` as a function rather than a constant

Rejected. `path.sep` is a compile-time constant. Exposing it as a function would imply it might change at runtime, which it cannot. Every other runtime (Node, Deno, Python) exposes the separator as a constant.

### Follow Lune's approach (no path module, raw strings)

Rejected. Lune's absence of a path module is a known gap that forces authors to manipulate separators manually. neoc targets professional-grade scripting use-cases where cross-platform correctness is not optional.

## Open questions

1. **`lexical_normalize` implementation.** No Rust standard library function performs lexical path normalization without filesystem access (only `canonicalize` exists, which requires filesystem). The implementation should use the `path-clean` crate or an equivalent in-tree implementation. If the team prefers no additional crates in `std:*`, an in-tree implementation is straightforward (~50 lines). Decision needed before the implementation PR.

2. **`path.join` variadic signature.** In Rust, `PathBuf::push` appends one component at a time; Node's `path.join` accepts variadic string arguments. The flat `path.join` follows Node. The `PathBuf:push` method follows Rust (single argument). This asymmetry is intentional but should be confirmed before landing.

3. **`path.relative` edge cases.** What does `path.relative` return when `from` and `to` are on different Windows drive letters? Go returns an error; Node returns the absolute `to` path. neoc should decide which behaviour to adopt. Recommendation: return the absolute `to` path (Node behaviour), as it is always valid even if not a relative path.

4. **Cross-type method calls.** Should `Path:join` accept a `PathBuf` argument in addition to a plain string? Currently specified as string-only. Accepting `PathBuf` would require the Rust implementation to accept a `mlua::AnyUserData` and downcast. Decision deferred to implementation.

5. **`PathBuf` equality with `Path`.** The `__eq` metamethod on `PathBuf` must compare equal to a `Path` with the same string representation. mlua's `__eq` is only invoked when both operands are the same UserData type. Cross-type equality may require a workaround (e.g., both implement `__tostring` and equality falls back to string comparison). This needs to be resolved in the implementation. Until resolved, cross-type equality is unspecified behaviour — do not depend on it.

## Implementation notes

- **Files:** `src/lua/std/path.rs` (module), registered in `src/lua/std/mod.rs` alphabetically.
- **Shared read-method helper:** Extract a `fn register_path_read_methods<T: UserData + AsRef<StdPath>>(methods: &mut UserDataMethods<T>)` to avoid duplicating `Path` methods on `PathBuf`.
- **Lexical normalize:** If no in-tree implementation, add `path-clean = "1"` (or equivalent) to `Cargo.toml` under a `[dependencies]` block. Document the decision in the PR.
- **`path.resolve` CWD dependency:** `path.resolve` calls `std::env::current_dir()` internally. If `current_dir()` fails (deleted working directory), surface the error via `error()` with the prefix `"path.resolve: current_dir unavailable: <reason>"`.
- **Tests:** `tests/std/path.test.luau` — cover all acceptance criteria. Separator-sensitive assertions should be conditional on `require("std:env").os`. Include test cases for: `path.basename("/")`, `path.extname(".gitignore")`, `path.normalize("/../foo")`, and all functions called with empty string `""`.
- **`.d.luau` stub:** `types/std/path.d.luau` — required per M1 Definition of Done. The `PathBuf = Path & { ... }` intersection type accurately models the inheritance relationship.
- **Criterion bench:** gated behind `bench` feature — include at minimum `join` (10-segment path) and `normalize` (path with multiple `..` components).
- **Sequencing:** `std:path` has no dependency on `std:fs` or `std:env` at the module level. `path.resolve` calls `std::env::current_dir()` directly (Rust stdlib, not neoc's `std:env`). The module can be picked up and landed independently. It does not block and is not blocked by any other open issue.
