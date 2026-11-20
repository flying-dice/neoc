//! A declarative facade over mlua module construction.
//!
//! A module is declared once, and that single declaration both registers
//! functions with mlua exactly as a hand-written `module()` would and records
//! the signature + doc for each function (and, for resource handles, each
//! method, field and constructor). The recording is dumped as a `LuaCATS`
//! definition file by [`Module::luals_meta`]: `cargo run -p neoc-vm --example luals`
//! writes one per module to `types/luals/`, and a lua-language-server plugin
//! makes `require("lib:json")` resolve to it, with completion and diagnostics. There is no reverse
//! mapping: the Rust source is the source of truth.
//!
//! Three things the facade preserves byte-for-byte versus the hand-written
//! `module()` functions it replaces:
//!
//! 1. **The `(value, error)` shape.** [`Module::try_func`] routes through
//!    [`crate::lua::sandbox::ok`] / [`crate::lua::sandbox::from_err`], so a
//!    fallible function returns `(v, nil)` on success and
//!    `(nil, "<stem>.<fn>: <e>")` on failure — the identical idiom and the
//!    identical error prefix the original used. The definition file says the
//!    same: `---@return T?` then `---@return string? err`.
//! 2. **Per-call tracing spans.** Every registered function opens a
//!    `tracing` span named `"<stem>.<name>"` at target
//!    `"lua::lib::<module>"` around the call body, matching the old
//!    `#[instrument(name = …, target = …, skip_all)]` attribute. See
//!    `dyn_span`.
//! 3. **The registered Lua surface.** The functions and userdata types land
//!    on the table exactly as `lua.create_function` / `lua.create_userdata`
//!    would place them.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Mutex, OnceLock};

use mlua::{FromLuaMulti, IntoLua, IntoLuaMulti, Lua, Table};
use tracing::{Instrument, Level};
use tracing_core::callsite::{Callsite, DefaultCallsite, Identifier};
use tracing_core::field::FieldSet;
use tracing_core::metadata::{Kind, Metadata};

use crate::lua::lifecycle::{Role, async_function};
use crate::lua::sandbox::{from_err, ok};

/// Maps a Rust binding type to its recorded type name.
///
/// Implemented for the bridge types a Lua-bound function takes or returns.
/// The associated name is the surface type, not the Rust type: every
/// string-shaped binding (`mlua::LuaString`, `String`, `&str`, `Vec<u8>`)
/// collapses to `"string"`, every numeric binding collapses to `"number"`,
/// and every dynamic value binding (`mlua::Value`, `mlua::MultiValue`,
/// `mlua::AnyUserData`) collapses to the opaque `"LuaValue"`.
pub trait DeclType {
    /// Returns the recorded type name for this binding type.
    fn decl_type() -> String;
}

macro_rules! decl_type {
    ($name:literal => $($ty:ty),+ $(,)?) => {
        $(impl DeclType for $ty {
            fn decl_type() -> String { $name.to_string() }
        })+
    };
}

decl_type!("string" => mlua::LuaString, String, &str, Vec<u8>);
decl_type!("number" => f64, f32, i64, i32, u64, u32, usize, isize);
decl_type!("bool" => bool);
decl_type!("()" => ());
decl_type!("LuaValue" => mlua::Value, mlua::MultiValue, mlua::AnyUserData, mlua::Table, mlua::Function);

impl<T: DeclType> DeclType for Option<T> {
    fn decl_type() -> String {
        format!("{}?", T::decl_type())
    }
}

impl<T: DeclType, E: DeclType> DeclType for Result<T, E> {
    fn decl_type() -> String {
        format!("Result<{}, {}>", T::decl_type(), E::decl_type())
    }
}

/// One recorded function: its name, parameter list (name + recorded type per
/// param), return type, and doc string. Enough to emit one annotated `LuaCATS`
/// function definition.
struct Entry {
    name: String,
    params: Vec<(String, String)>,
    ret: String,
    doc: String,
}

impl Entry {
    /// Emits the annotated, bodyless `LuaCATS` definition of this entry.
    ///
    /// `owner` is the local table the function hangs on and `method` picks
    /// `owner:name(...)` (an instance method, with the implicit `self`) over
    /// `owner.name(...)` (a free or associated function). `scope` is the class
    /// prefix that the module's own type names resolve under.
    fn write_luals(&self, out: &mut String, scope: &str, owner: &str, method: bool) {
        write_doc(out, &self.doc);
        for (name, ty) in &self.params {
            if name == VARIADIC {
                out.push_str("---@param ... any\n");
                continue;
            }
            let (ty, optional) = luals_param_type(ty, scope);
            out.push_str(&format!(
                "---@param {}{} {ty}\n",
                luals_ident(name),
                if optional { "?" } else { "" }
            ));
        }
        for ret in luals_returns(&self.ret, scope) {
            out.push_str(&format!("---@return {ret}\n"));
        }
        let params = self
            .params
            .iter()
            .map(|(n, _)| {
                if n == VARIADIC {
                    VARIADIC.to_string()
                } else {
                    luals_ident(n)
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let sep = if method { ':' } else { '.' };
        out.push_str(&format!(
            "function {owner}{sep}{}({params}) end\n\n",
            luals_ident(&self.name)
        ));
    }
}

/// The recorded name of a trailing variadic parameter. A function that takes
/// any number of further arguments ends its `params` with
/// `("...", "LuaValue")`, which the definitions write as `---@param ... any`
/// and `function f(a, ...)`.
const VARIADIC: &str = "...";

/// Writes `doc` as `---` comment lines (nothing when it is empty).
fn write_doc(out: &mut String, doc: &str) {
    for line in doc.lines() {
        out.push_str(&format!("--- {line}\n"));
    }
}

/// Reserved words of Lua 5.1 and of Lua 5.2+ (`goto`, which the editor tooling may be configured for; Luau treats it
/// as an identifier): not usable as a
/// parameter or function name in a definition file.
const LUA_KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in",
    "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];

/// A name made safe to write as a Lua identifier: a reserved word gets a
/// trailing underscore.
fn luals_ident(name: &str) -> String {
    if LUA_KEYWORDS.contains(&name) {
        format!("{name}_")
    } else {
        name.to_string()
    }
}

/// The `LuaCATS` type of one recorded type string, and whether it is optional.
///
/// The recorded vocabulary is the facade's own (see [`DeclType`]): `string`,
/// `number`, `bool`, `LuaValue` (the opaque dynamic value), a trailing `?` for
/// an optional, and any other name for a type this module declares, which
/// resolves under `scope`. A name with a dot (the class of another module), a
/// `fun(...)` type or a parenthesised type is written as it is. A `fun(...)` type
/// is never optional by a trailing `?`: that `?` belongs to its return type
/// (`fun(a): string?`); write `(fun(a): b)?` for an optional function.
fn luals_param_type(ty: &str, scope: &str) -> (String, bool) {
    let ty = luals_type(ty, scope);
    if ty.starts_with("fun(") {
        return (ty, false);
    }
    match ty.strip_suffix('?') {
        Some(base) => (base.to_string(), true),
        None => (ty, false),
    }
}

fn luals_type(ty: &str, scope: &str) -> String {
    match ty.trim() {
        "string" => "string".to_string(),
        "number" => "number".to_string(),
        "bool" => "boolean".to_string(),
        "LuaValue" => "any".to_string(),
        "()" => "nil".to_string(),
        VARIADIC => "any ...".to_string(),
        // a class of another module (`std_net.TcpListener`) or a function type is written as it is
        other if other.contains('.') || other.starts_with("fun(") || other.starts_with('(') => {
            other.to_string()
        }
        other => match other.strip_suffix('?') {
            Some(base) => format!("{}?", luals_type(base, scope)),
            None => format!("{scope}.{other}"),
        },
    }
}

/// The `---@return` lines for a recorded return type.
///
/// `()` returns nothing. `Result<T, string>` is the runtime's `(value, error)`
/// idiom: `T?` then `string? err` (a `()` value is `nil`). `(A, B)` is a
/// function that returns several values of its own, one `---@return` each, and
/// a `...` in it (or alone) is any number of further values (`---@return any ...`),
/// for a function that hands back whatever a Lua function returned. Anything else
/// is one value.
fn luals_returns(ret: &str, scope: &str) -> Vec<String> {
    let ret = ret.trim();
    if ret == "()" {
        return Vec::new();
    }
    if let Some(values) = ret.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
        return split_top_level(values)
            .into_iter()
            .map(|v| luals_type(v, scope))
            .collect();
    }
    if let Some(value) = ret
        .strip_prefix("Result<")
        .and_then(|r| r.strip_suffix(", string>"))
    {
        let value = luals_type(value, scope);
        let value = if value == "nil" || value == "any" || value.ends_with('?') {
            value
        } else {
            format!("{value}?")
        };
        return vec![value, "string? err".to_string()];
    }
    vec![luals_type(ret, scope)]
}

/// Splits `s` on the commas that are not inside brackets, so a `fun(a, b)` or
/// `table<string, any>` type stays in one piece.
fn split_top_level(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0_usize;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// The class name a registry key's module is declared under: `lib:json`
/// becomes `lib_json`.
fn luals_class(name: &str) -> String {
    name.replace(':', "_")
}

/// One recorded resource-handle userdata type. The receiver (`self`) is
/// implicit on every method, so only the trailing arguments are recorded.
/// `fields` are the read-only values (`date.year`), `statics` and
/// `static_consts` the members of the same-named sub-table on the module table
/// (`Duration.from_secs`, `Duration.ZERO`).
struct UserDataEntry {
    type_name: String,
    methods: Vec<Entry>,
    functions: Vec<Entry>,
    fields: Vec<FieldEntry>,
    statics: Vec<Entry>,
    static_consts: Vec<FieldEntry>,
}

/// One recorded `struct <Type>; impl <Type> { … }` for a constructor-bearing
/// type that the module exposes as a sub-table (for example
/// `std:collections`'s `Map`, reached as `Map.new()`). Constructors are
/// associated functions (no `self`); methods carry an implicit `self`.
struct TypeEntry {
    type_name: String,
    constructors: Vec<Entry>,
    methods: Vec<Entry>,
}

/// One recorded value that is not a function: a module-level constant (for
/// example `std:thread`'s dynamically-injected `parent` / `shared`), a userdata
/// type's read-only field (`date.year`) or a constant on a type's sub-table
/// (`Duration.ZERO`). It is emitted as a `---@field`.
struct FieldEntry {
    name: String,
    ty: String,
    doc: String,
}

/// Builds a Lua module table while recording the signature of each function
/// and userdata type it registers.
///
/// Register free functions with [`Module::func`] (infallible) and
/// [`Module::try_func`] (fallible); both accept any arity through
/// `FromLuaMulti` plus an explicit parameter-name/type slice. Record a
/// resource-handle userdata type with [`Module::userdata`]. When the surface
/// is complete, hand the table to the engine with [`Module::into_table`] and
/// emit the matching `LuaCATS` definitions with [`Module::luals_meta`].
pub struct Module<'l> {
    lua: &'l Lua,
    table: Table,
    name: String,
    entries: Vec<Entry>,
    userdata: Vec<UserDataEntry>,
    types: Vec<TypeEntry>,
    consts: Vec<FieldEntry>,
}

impl<'l> Module<'l> {
    /// Creates a new module builder with an empty backing table.
    ///
    /// The `name` is the registry key the module is installed under
    /// (for example, `"lib:base64"`) and names the module's definition file.
    pub fn new(lua: &'l Lua, name: impl Into<String>) -> mlua::Result<Self> {
        Ok(Self {
            lua,
            table: lua.create_table()?,
            name: name.into(),
            entries: Vec::new(),
            userdata: Vec::new(),
            types: Vec::new(),
            consts: Vec::new(),
        })
    }

    /// Registers an infallible function of any arity.
    ///
    /// `params` is the ordered list of `(parameter_name, decl_type)` pairs for
    /// the function's signature; the runtime argument type is the single
    /// `FromLuaMulti` type `A` (use a tuple for multiple arguments, matching
    /// the hand-written `(a, b): (TypeA, TypeB)` style). `f` is wrapped in
    /// `lua.create_function`, set on the table under `name`, and its body runs
    /// inside a `tracing` span named `"<stem>.<name>"` so the call is traced
    /// exactly as the old `#[instrument]` attribute did. The recorded
    /// signature is `fn <name>(<params>) -> R`, where `R` resolves through
    /// [`DeclType`].
    pub fn func<A, R>(
        &mut self,
        name: &str,
        params: &[(&str, &str)],
        doc: &str,
        f: impl Fn(&Lua, A) -> mlua::Result<R> + Send + 'static,
    ) -> mlua::Result<&mut Self>
    where
        A: FromLuaMulti,
        R: DeclType + IntoLua,
    {
        let span = dyn_span(
            &self.target(),
            &format!("{}.{}", module_stem(&self.name), name),
        );
        self.table.set(
            name,
            self.lua
                .create_function(move |lua, a: A| span.in_scope(|| f(lua, a)))?,
        )?;
        self.entries.push(Entry {
            name: name.to_string(),
            params: own_params(params),
            ret: R::decl_type(),
            doc: doc.to_string(),
        });
        Ok(self)
    }

    /// Registers a fallible function of any arity.
    ///
    /// `f` returns `Result<T, E>`. The result is marshalled to the runtime's
    /// `(value, error)` idiom: `Ok(v)` becomes `(v, nil)` and `Err(e)`
    /// becomes `(nil, "<stem>.<name>: <e>")`, reusing
    /// [`crate::lua::sandbox::ok`] and [`crate::lua::sandbox::from_err`] so the
    /// behaviour — including the error prefix — is identical to a hand-written
    /// fallible function. The body runs inside the same `"<stem>.<name>"`
    /// tracing span as [`Module::func`]. The error channel is always a string
    /// in this runtime, so the recorded return type is `Result<<T>, string>`.
    pub fn try_func<A, T, E>(
        &mut self,
        name: &str,
        params: &[(&str, &str)],
        doc: &str,
        f: impl Fn(&Lua, A) -> Result<T, E> + Send + 'static,
    ) -> mlua::Result<&mut Self>
    where
        A: FromLuaMulti,
        T: DeclType + IntoLua,
        E: std::fmt::Display,
    {
        let prefix = format!("{}.{}", module_stem(&self.name), name);
        let span = dyn_span(&self.target(), &prefix);
        let func = self.lua.create_function(move |lua, a: A| {
            span.in_scope(|| match f(lua, a) {
                Ok(v) => ok(lua, v),
                Err(e) => from_err(lua, &prefix, e),
            })
        })?;
        self.table.set(name, func)?;
        self.entries.push(Entry {
            name: name.to_string(),
            params: own_params(params),
            ret: format!("Result<{}, string>", T::decl_type()),
            doc: doc.to_string(),
        });
        Ok(self)
    }

    /// Registers an infallible async function of any arity.
    ///
    /// The async counterpart of [`Module::func`]: `f` returns a future, and
    /// the function is made by [`async_function`] (a [`Role::Wait`]) so calls yield
    /// the calling Lua coroutine until the future resolves. The future runs
    /// inside the same `"<stem>.<name>"` tracing span [`Module::func`] uses,
    /// attached via [`tracing::Instrument`] so the span covers the whole
    /// `await`. The definitions have no async surface — async is a runtime detail — so the
    /// recorded signature is identical to a synchronous `fn`:
    /// `fn <name>(<params>) -> R`.
    pub fn async_func<A, R, Fut>(
        &mut self,
        name: &str,
        params: &[(&str, &str)],
        doc: &str,
        f: impl Fn(Lua, A) -> Fut + Send + 'static,
    ) -> mlua::Result<&mut Self>
    where
        A: FromLuaMulti + 'static,
        R: DeclType + IntoLuaMulti + Send + 'static,
        Fut: Future<Output = mlua::Result<R>> + Send + 'static,
    {
        let span = dyn_span(
            &self.target(),
            &format!("{}.{}", module_stem(&self.name), name),
        );
        let func = async_function(self.lua, Role::Wait, move |lua, a: A| {
            f(lua, a).instrument(span.clone())
        })?;
        self.table.set(name, func)?;
        self.entries.push(Entry {
            name: name.to_string(),
            params: own_params(params),
            ret: R::decl_type(),
            doc: doc.to_string(),
        });
        Ok(self)
    }

    /// Registers a fallible async function of any arity.
    ///
    /// The async counterpart of [`Module::try_func`]: `f` returns a future
    /// resolving to `Result<T, E>`, marshalled to the runtime's
    /// `(value, error)` idiom exactly as [`Module::try_func`] does — `Ok(v)`
    /// becomes `(v, nil)` and `Err(e)` becomes `(nil, "<stem>.<name>: <e>")`,
    /// with the identical error prefix. The future runs inside the same
    /// `"<stem>.<name>"` tracing span, attached via [`tracing::Instrument`].
    /// The recorded return type is `Result<<T>, string>`, identical to
    /// the synchronous form.
    pub fn try_async_func<A, T, E, Fut>(
        &mut self,
        name: &str,
        params: &[(&str, &str)],
        doc: &str,
        f: impl Fn(Lua, A) -> Fut + Send + 'static,
    ) -> mlua::Result<&mut Self>
    where
        A: FromLuaMulti + 'static,
        T: DeclType + IntoLua,
        E: std::fmt::Display,
        Fut: Future<Output = Result<T, E>> + Send + 'static,
    {
        let prefix = format!("{}.{}", module_stem(&self.name), name);
        let span = dyn_span(&self.target(), &prefix);
        // a plain wait: building the result's Lua values needs no turn (see `Role`)
        let func = async_function(self.lua, Role::Wait, move |lua, a: A| {
            let fut = f(lua.clone(), a).instrument(span.clone());
            let prefix = prefix.clone();
            async move {
                match fut.await {
                    Ok(v) => ok(&lua, v),
                    Err(e) => from_err(&lua, &prefix, e),
                }
            }
        })?;
        self.table.set(name, func)?;
        self.entries.push(Entry {
            name: name.to_string(),
            params: own_params(params),
            ret: format!("Result<{}, string>", T::decl_type()),
            doc: doc.to_string(),
        });
        Ok(self)
    }

    /// Sets an already-built value on the module table without recording a
    /// signature.
    ///
    /// For surface that the facade does not model — a constructor sub-table
    /// (`mod.Type.new`), a sentinel userdata, or any value built outside the
    /// `func` / `try_func` path. The registration is byte-identical to a
    /// hand-written `t.set(name, value)`.
    pub fn set(&mut self, name: &str, value: impl IntoLua) -> mlua::Result<&mut Self> {
        self.table.set(name, value)?;
        Ok(self)
    }

    /// Records a free-function signature without registering an
    /// implementation.
    ///
    /// The signature-only counterpart to [`Module::set`]: pair the two when a
    /// free function must be registered verbatim — because it raises an
    /// `mlua::Error` directly, bakes its own error prefix, shares a prefix
    /// helper with a userdata method, or runs under a custom or absent
    /// `tracing` span — yet must still appear in the recorded surface. `params`
    /// is the ordered list of `(parameter_name, decl_type)` pairs; `ret` is the
    /// return type as a string (the caller passes it directly because such
    /// a function's return shape is decided in its body rather than by a single
    /// Rust return type, exactly as [`UserDataSpec::method`] does). This records
    /// only the recorded surface; the runtime registration is whatever
    /// [`Module::set`] installed, left untouched.
    pub fn record_fn(
        &mut self,
        name: &str,
        params: &[(&str, &str)],
        ret: &str,
        doc: &str,
    ) -> &mut Self {
        self.entries.push(Entry {
            name: name.to_string(),
            params: own_params(params),
            ret: ret.to_string(),
            doc: doc.to_string(),
        });
        self
    }

    /// Records a resource-handle userdata type for the emitted definitions.
    ///
    /// `type_name` is the type name the userdata is recorded under (matching
    /// the Rust type name, e.g. `"Object"`). The closure receives a
    /// [`UserDataSpec`] recorder; declare each method on it with
    /// [`UserDataSpec::method`], each read-only value with
    /// [`UserDataSpec::field`], and the functions and constants of the type's
    /// sub-table on the module with [`UserDataSpec::static_fn`] and
    /// [`UserDataSpec::static_const`]. This only records the surface — the
    /// runtime registration of the type's methods stays in the type's own
    /// `impl UserData`, untouched, so its behaviour is byte-identical to today.
    pub fn userdata(
        &mut self,
        type_name: &str,
        build: impl FnOnce(&mut UserDataSpec),
    ) -> &mut Self {
        let mut spec = UserDataSpec {
            entry: UserDataEntry {
                type_name: type_name.to_string(),
                methods: Vec::new(),
                functions: Vec::new(),
                fields: Vec::new(),
                statics: Vec::new(),
                static_consts: Vec::new(),
            },
        };
        build(&mut spec);
        self.userdata.push(spec.entry);
        self
    }

    /// Registers a constructor-bearing type exposed as a sub-table.
    ///
    /// Models the pattern where a module surfaces a type through a sub-table
    /// holding one or more constructors — `std:collections`'s `Map.new()` /
    /// `Counter.new(initial)`, `std:net`'s `TcpListener.bind(addr)`. The
    /// closure receives a [`TypeSpec`]: declare each constructor with
    /// [`TypeSpec::constructor`] (it both registers the function on the
    /// sub-table with mlua and records an associated `fn`) and each instance
    /// method with [`TypeSpec::method`] (recorded only — the method's runtime
    /// registration stays in the type's own `impl UserData`). The sub-table is
    /// created here, populated with the constructors, and set on the module
    /// table under `type_name`, exactly as a hand-written `t.set("Map", map_t)`
    /// would. The recorded definitions are a `<Type>` class of methods and a
    /// `<Type>Statics` class of constructors.
    pub fn type_(
        &mut self,
        type_name: &str,
        build: impl FnOnce(&mut TypeSpec) -> mlua::Result<()>,
    ) -> mlua::Result<&mut Self> {
        let sub = self.lua.create_table()?;
        let mut spec = TypeSpec {
            lua: self.lua,
            sub: &sub,
            target: self.target(),
            type_name: type_name.to_string(),
            constructors: Vec::new(),
            methods: Vec::new(),
        };
        build(&mut spec)?;
        let entry = TypeEntry {
            type_name: spec.type_name,
            constructors: spec.constructors,
            methods: spec.methods,
        };
        self.table.set(type_name, sub)?;
        self.types.push(entry);
        Ok(self)
    }

    /// Records a module-level constant for the emitted definitions.
    ///
    /// For a value bound directly on the module table whose shape the facade
    /// does not model as a function — most notably a dynamically-injected
    /// global such as `std:thread`'s `parent` / `shared`, which only exists in
    /// a spawned worker and cannot be typed beyond the opaque `LuaValue`. This
    /// only records the surface (a `---@field` on the module class); the
    /// runtime value is set elsewhere (or injected at spawn time), so the
    /// registration is unchanged.
    pub fn const_(&mut self, name: &str, ty: &str, doc: &str) -> &mut Self {
        self.consts.push(FieldEntry {
            name: name.to_string(),
            ty: ty.to_string(),
            doc: doc.to_string(),
        });
        self
    }

    /// Consumes the builder and returns the backing Lua table for
    /// registration by `module()`.
    pub fn into_table(self) -> Table {
        self.table
    }

    /// The registry key this module is installed under (for example,
    /// `"lib:base64"`).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The `tracing` target for this module's spans.
    ///
    /// Every runtime module — regardless of namespace — traces under
    /// `"lua::lib::<module>"`, matching the existing hand-written
    /// `#[instrument(target = …)]` attributes (a `std:collections` registry
    /// key still traces at `"lua::lib::collections"`). The namespace is a
    /// registration concern, not part of the trace target.
    fn target(&self) -> String {
        format!("lua::lib::{}", module_stem(&self.name))
    }

    /// Emits the `LuaCATS` definition file for this module: `---@meta`, then
    /// one class per resource type and one for the module table, then the
    /// module table's functions.
    ///
    /// The caller writes it to the path [`crate::lua::modules::luals_file`] gives
    /// for the registry key (`lib/json.lua`). Classes are `<ns>_<stem>` for the module (`lib_json`) and
    /// `<ns>_<stem>.<Type>` for each type it declares; a type with a sub-table
    /// of functions or constants on the module (`Duration.from_secs`,
    /// `Duration.ZERO`) also gets a `<Type>Statics` class, the type of that
    /// field of the module.
    ///
    /// # Errors
    /// Fails when the live module table cannot be read, or when a type's
    /// sub-table on it disagrees with the recording: a member that was not
    /// recorded would be a false alarm for every script that uses it, and a
    /// recorded one that does not exist a call the editor accepts and the VM
    /// refuses.
    pub fn luals_meta(&self) -> mlua::Result<String> {
        let class = luals_class(&self.name);
        let mut out = String::new();
        out.push_str("---@meta\n\n");
        out.push_str(
            "-- GENERATED by neoc-vm (`cargo run -p neoc-vm --example luals`) from the module's\n",
        );
        out.push_str(
            "-- recorded surface. Do not edit. Regenerate with the same command; `-- --check` fails\n",
        );
        out.push_str("-- when this file is stale.\n--\n");
        out.push_str(&format!(
            "-- Definitions for lua-language-server: `require(\"{}\")`. Never loaded by neoc.\n\n",
            self.name
        ));

        let mut module_fields = Vec::new();
        for ud in &self.userdata {
            write_class(
                &mut out,
                &class,
                &ud.type_name,
                &ud.fields,
                &[&ud.methods, &ud.functions],
            );
            self.check_statics_recorded(ud)?;
            if !ud.statics.is_empty() || !ud.static_consts.is_empty() {
                write_statics(
                    &mut out,
                    &class,
                    &ud.type_name,
                    &ud.static_consts,
                    &ud.statics,
                );
                module_fields.push(&ud.type_name);
            }
        }
        for ty in &self.types {
            write_class(&mut out, &class, &ty.type_name, &[], &[&ty.methods, &[]]);
            write_statics(&mut out, &class, &ty.type_name, &[], &ty.constructors);
            module_fields.push(&ty.type_name);
        }

        out.push_str(&format!("---@class {class}\n"));
        for name in module_fields {
            out.push_str(&format!("---@field {name} {class}.{name}Statics\n"));
        }
        for c in &self.consts {
            write_field(&mut out, &class, c);
        }
        out.push_str("local M = {}\n\n");
        for e in &self.entries {
            e.write_luals(&mut out, &class, "M", false);
        }
        out.push_str(&format!("---@type {class}\nreturn M\n"));
        Ok(out)
    }

    /// What is recorded for each resource type of the module:
    /// `(type, methods, fields)`, so a test can hold the records to a live
    /// instance (a recorded method the type does not have would be a call
    /// `LuaLS` accepts and the VM refuses).
    #[cfg(test)]
    pub(crate) fn recorded_members(&self) -> Vec<(String, Vec<String>, Vec<String>)> {
        let names = |entries: &[Entry]| entries.iter().map(|e| e.name.clone()).collect::<Vec<_>>();
        let userdata = self.userdata.iter().map(|ud| {
            let fields = ud.fields.iter().map(|f| f.name.clone()).collect();
            let methods = names(&ud.methods).into_iter().chain(names(&ud.functions));
            (ud.type_name.clone(), methods.collect(), fields)
        });
        let types = self
            .types
            .iter()
            .map(|ty| (ty.type_name.clone(), names(&ty.methods), Vec::new()));
        userdata.chain(types).collect()
    }

    /// Fails when the live sub-table for `ud` on the module table and the
    /// type's recorded statics (functions and constants) disagree, in either
    /// direction.
    fn check_statics_recorded(&self, ud: &UserDataEntry) -> mlua::Result<()> {
        let recorded = |key: &str| {
            ud.statics.iter().any(|e| e.name == key)
                || ud.static_consts.iter().any(|c| c.name == key)
        };
        let mlua::Value::Table(sub) = self.table.get::<mlua::Value>(ud.type_name.as_str())? else {
            return if recorded_any(ud) {
                Err(mlua::Error::external(format!(
                    "{}: `{}` records statics but the module has no such sub-table",
                    self.name, ud.type_name
                )))
            } else {
                Ok(())
            };
        };
        let mut live = Vec::new();
        for pair in sub.pairs::<String, mlua::Value>() {
            let (key, _) = pair?;
            if !recorded(&key) {
                return Err(mlua::Error::external(format!(
                    "{}: {}.{key} is on the module table but not recorded (add it with static_fn or static_const)",
                    self.name, ud.type_name
                )));
            }
            live.push(key);
        }
        let names = ud
            .statics
            .iter()
            .map(|e| &e.name)
            .chain(ud.static_consts.iter().map(|c| &c.name));
        for name in names {
            if !live.contains(name) {
                return Err(mlua::Error::external(format!(
                    "{}: {}.{name} is recorded but the module table has no such member",
                    self.name, ud.type_name
                )));
            }
        }
        Ok(())
    }
}

/// Whether `ud` records any static function or constant.
fn recorded_any(ud: &UserDataEntry) -> bool {
    !ud.statics.is_empty() || !ud.static_consts.is_empty()
}

/// One `---@class <class>.<Type>` of values (`fields`), methods (`:`, with the
/// implicit `self`) and plain functions (`.`), hung on a local `ud_<Type>`.
fn write_class(
    out: &mut String,
    class: &str,
    type_name: &str,
    fields: &[FieldEntry],
    members: &[&[Entry]; 2],
) {
    out.push_str(&format!("---@class {class}.{type_name}\n"));
    for field in fields {
        write_field(out, class, field);
    }
    out.push_str(&format!("local ud_{type_name} = {{}}\n\n"));
    for m in members[0] {
        m.write_luals(out, class, &format!("ud_{type_name}"), true);
    }
    for f in members[1] {
        f.write_luals(out, class, &format!("ud_{type_name}"), false);
    }
}

/// One `---@class <class>.<Type>Statics`: the sub-table of a type on its module
/// (`Duration.from_secs`).
fn write_statics(
    out: &mut String,
    class: &str,
    type_name: &str,
    consts: &[FieldEntry],
    functions: &[Entry],
) {
    out.push_str(&format!("---@class {class}.{type_name}Statics\n"));
    for c in consts {
        write_field(out, class, c);
    }
    out.push_str(&format!("local statics_{type_name} = {{}}\n\n"));
    for f in functions {
        f.write_luals(out, class, &format!("statics_{type_name}"), false);
    }
}

/// One `---@field <name> <type> <doc>` line.
fn write_field(out: &mut String, class: &str, field: &FieldEntry) {
    let (ty, optional) = luals_param_type(&field.ty, class);
    let mark = if optional { "?" } else { "" };
    let doc = field.doc.replace('\n', " ");
    out.push_str(&format!(
        "---@field {}{mark} {ty} {doc}\n",
        luals_ident(&field.name)
    ));
}

/// Records the surface of one resource-handle userdata type (or of a plain
/// table that acts like one).
///
/// Handed to the closure passed to [`Module::userdata`]. Each
/// [`UserDataSpec::method`] call mirrors one `add_method` on the type's
/// `impl UserData`, each [`UserDataSpec::field`] one `add_field_method_get`,
/// and [`UserDataSpec::static_fn`] / [`UserDataSpec::static_const`] the
/// members of the type's sub-table on the module. The receiver (`self`) is
/// implicit on methods and is not listed in `params`.
pub struct UserDataSpec {
    entry: UserDataEntry,
}

impl UserDataSpec {
    /// Records one method on the userdata type.
    ///
    /// `params` is the ordered list of `(parameter_name, decl_type)` pairs for
    /// the arguments *after* the implicit `self` receiver. `ret` is the
    /// return type as a string (callers pass it directly because a method's
    /// return shape — often `LuaValue` or `(LuaValue, LuaValue)` — is decided
    /// at the `add_method` body rather than by a single Rust return type). A
    /// method that takes any number of trailing arguments ends `params` with
    /// `("...", "LuaValue")`.
    pub fn method(
        &mut self,
        name: &str,
        params: &[(&str, &str)],
        ret: &str,
        doc: &str,
    ) -> &mut Self {
        self.entry.methods.push(Entry {
            name: name.to_string(),
            params: own_params(params),
            ret: ret.to_string(),
            doc: doc.to_string(),
        });
        self
    }

    /// Records one function stored on a value of the type, called with a dot
    /// and no receiver (`value.toBe(x)`): for a plain table of closures such as
    /// `lib:test`'s matcher, which is not a userdata. `params` and `ret` are as
    /// for [`UserDataSpec::method`].
    pub fn function(
        &mut self,
        name: &str,
        params: &[(&str, &str)],
        ret: &str,
        doc: &str,
    ) -> &mut Self {
        self.entry.functions.push(Entry {
            name: name.to_string(),
            params: own_params(params),
            ret: ret.to_string(),
            doc: doc.to_string(),
        });
        self
    }

    /// Records one read-only field on the userdata type (`add_field_method_get`):
    /// `date.year`, not `date:year()`. `ty` is the recorded type of the value.
    pub fn field(&mut self, name: &str, ty: &str, doc: &str) -> &mut Self {
        self.entry.fields.push(FieldEntry {
            name: name.to_string(),
            ty: ty.to_string(),
            doc: doc.to_string(),
        });
        self
    }

    /// Records one function on the type's sub-table of the module table
    /// (`Duration.from_secs(n)`): a constructor or another function with no
    /// `self`. `params` and `ret` are as for [`UserDataSpec::method`].
    pub fn static_fn(
        &mut self,
        name: &str,
        params: &[(&str, &str)],
        ret: &str,
        doc: &str,
    ) -> &mut Self {
        self.entry.statics.push(Entry {
            name: name.to_string(),
            params: own_params(params),
            ret: ret.to_string(),
            doc: doc.to_string(),
        });
        self
    }

    /// Records one constant value on the type's sub-table of the module table
    /// (`Duration.ZERO`).
    pub fn static_const(&mut self, name: &str, ty: &str, doc: &str) -> &mut Self {
        self.entry.static_consts.push(FieldEntry {
            name: name.to_string(),
            ty: ty.to_string(),
            doc: doc.to_string(),
        });
        self
    }
}

/// Records (and registers) the constructor + method surface of one
/// constructor-bearing type exposed as a sub-table.
///
/// Handed to the closure passed to [`Module::type_`]. Constructors are
/// associated functions registered on the sub-table with mlua and recorded as
/// `fn`s with no `self`. Methods are recorded only — their
/// runtime registration stays in the type's `impl UserData`, untouched — and
/// carry an implicit `self` receiver not listed in `params`.
pub struct TypeSpec<'l, 't> {
    lua: &'l Lua,
    sub: &'t Table,
    target: String,
    type_name: String,
    constructors: Vec<Entry>,
    methods: Vec<Entry>,
}

impl TypeSpec<'_, '_> {
    /// Registers a constructor on the type's sub-table and records it.
    ///
    /// `name` is the constructor (`"new"`, `"bind"`, …); it is set on the
    /// sub-table via `lua.create_function`, so `Type.<name>(args)` calls it,
    /// byte-identically to a hand-written `sub.set("new", …)`. The body runs
    /// inside a `tracing` span named `"<Type>.<name>"` at the type's module
    /// target, matching the old `#[instrument(name = "Type.new", …)]`
    /// attribute on the constructor function. `params` is the ordered list of
    /// `(parameter_name, decl_type)` pairs; the recorded associated function
    /// is `fn <name>(<params>) -> <Type>` (constructors return the type).
    pub fn constructor<A, R>(
        &mut self,
        name: &str,
        params: &[(&str, &str)],
        doc: &str,
        f: impl Fn(&Lua, A) -> mlua::Result<R> + Send + 'static,
    ) -> mlua::Result<&mut Self>
    where
        A: FromLuaMulti,
        R: IntoLua,
    {
        let span = dyn_span(&self.target, &format!("{}.{}", self.type_name, name));
        self.sub.set(
            name,
            self.lua
                .create_function(move |lua, a: A| span.in_scope(|| f(lua, a)))?,
        )?;
        self.constructors.push(Entry {
            name: name.to_string(),
            params: own_params(params),
            ret: self.type_name.clone(),
            doc: doc.to_string(),
        });
        Ok(self)
    }

    /// Registers a fallible constructor on the type's sub-table and records it.
    ///
    /// The fallible counterpart of [`TypeSpec::constructor`]: `f` returns
    /// `Result<T, E>`, marshalled to the runtime's `(value, error)` idiom —
    /// `Ok(v)` becomes `(v, nil)` and `Err(e)` becomes
    /// `(nil, "<Type>.<name>: <e>")` — reusing [`crate::lua::sandbox::ok`] and
    /// [`crate::lua::sandbox::from_err`] with the `"<Type>.<name>"` error
    /// prefix, identical to the hand-written constructor. The body runs inside
    /// the same `"<Type>.<name>"` tracing span as [`TypeSpec::constructor`].
    /// The recorded associated function is
    /// `fn <name>(<params>) -> Result<<Type>, string>`.
    pub fn try_constructor<A, T, E>(
        &mut self,
        name: &str,
        params: &[(&str, &str)],
        doc: &str,
        f: impl Fn(&Lua, A) -> Result<T, E> + Send + 'static,
    ) -> mlua::Result<&mut Self>
    where
        A: FromLuaMulti,
        T: IntoLua,
        E: std::fmt::Display,
    {
        let prefix = format!("{}.{}", self.type_name, name);
        let span = dyn_span(&self.target, &prefix);
        self.sub.set(
            name,
            self.lua.create_function(move |lua, a: A| {
                span.in_scope(|| match f(lua, a) {
                    Ok(v) => ok(lua, v),
                    Err(e) => from_err(lua, &prefix, e),
                })
            })?,
        )?;
        self.constructors.push(Entry {
            name: name.to_string(),
            params: own_params(params),
            ret: format!("Result<{}, string>", self.type_name),
            doc: doc.to_string(),
        });
        Ok(self)
    }

    /// Records one instance method on the type.
    ///
    /// Mirrors [`UserDataSpec::method`]: `params` lists the arguments after
    /// the implicit `self` receiver, and `ret` is the return type as a
    /// string. This only records the surface — the method's runtime
    /// registration lives in the type's own `impl UserData`.
    pub fn method(
        &mut self,
        name: &str,
        params: &[(&str, &str)],
        ret: &str,
        doc: &str,
    ) -> &mut Self {
        self.methods.push(Entry {
            name: name.to_string(),
            params: own_params(params),
            ret: ret.to_string(),
            doc: doc.to_string(),
        });
        self
    }
}

/// Clones a borrowed `(name, type)` slice into owned `String` pairs.
fn own_params(params: &[(&str, &str)]) -> Vec<(String, String)> {
    params
        .iter()
        .map(|(n, t)| (n.to_string(), t.to_string()))
        .collect()
}

/// Strips the namespace prefix from a registry key for use in error prefixes
/// and span names: `"lib:base64"` becomes `"base64"`. The namespace is a
/// registration concern, not part of the runtime error identity (see the
/// style guide).
fn module_stem(name: &str) -> &str {
    name.split_once(':').map_or(name, |(_, stem)| stem)
}

// ---------------------------------------------------------------------------
// Dynamic tracing spans
//
// The old per-function `#[instrument(name = "<stem>.<fn>", target =
// "lua::lib::<mod>", skip_all)]` froze its name + target into a static
// callsite at the attribute's source location. The facade registers every
// function from one source location, so the same name/target pair has to be
// chosen at runtime instead. `tracing`'s macros can't do that — they cache a
// single static callsite per call site — so we build the `Metadata` by hand.
//
// `Metadata` wants a `&'static str` name + target and a `FieldSet`. We have
// no fields (`skip_all`), so every span shares one process-wide empty
// `FieldSet` keyed off a single shared callsite; only the name + target vary.
// Those strings, and the per-pair `Metadata`, are interned once and reused for
// every later span of the same `(target, name)` — the set is bounded by the
// number of declared functions, so this is a fixed one-time cost, not a
// per-call leak.
// ---------------------------------------------------------------------------

/// The shared callsite whose identifier backs every dynamic span's (empty)
/// field set. Its own metadata is never surfaced — only its `Identifier` is
/// used, and an empty value set is empty regardless of which callsite it
/// names.
fn shared_callsite() -> &'static DefaultCallsite {
    static META: Metadata<'static> = Metadata::new(
        "neoc.facade",
        "lua",
        Level::INFO,
        Some(file!()),
        Some(line!()),
        Some(module_path!()),
        FieldSet::new(&[], Identifier(&SHARED)),
        Kind::SPAN,
    );
    static SHARED: DefaultCallsite = DefaultCallsite::new(&META);
    &SHARED
}

/// Per-`(target, name)` interned `Metadata`, so repeated declarations (every
/// engine boot re-runs `declare`) reuse one leaked record instead of leaking
/// on every call.
fn metadata_cache() -> &'static Mutex<HashMap<(String, String), &'static Metadata<'static>>> {
    static CACHE: OnceLock<Mutex<HashMap<(String, String), &'static Metadata<'static>>>> =
        OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Builds (or fetches the interned) `INFO`-level span `Metadata` for the
/// given target + name, then returns an unentered [`tracing::Span`] over it.
/// Entering the returned span around a call body reproduces the old
/// `#[instrument]` span: same name, same `lua::…` target, no fields.
fn dyn_span(target: &str, name: &str) -> tracing::Span {
    let key = (target.to_string(), name.to_string());
    let meta = {
        let mut cache = metadata_cache().lock().expect("facade metadata cache");
        *cache.entry(key).or_insert_with(|| {
            let name: &'static str = Box::leak(name.to_string().into_boxed_str());
            let target: &'static str = Box::leak(target.to_string().into_boxed_str());
            let fields = FieldSet::new(&[], shared_callsite().metadata().callsite());
            let meta = Metadata::new(
                name,
                target,
                Level::INFO,
                None,
                None,
                None,
                fields,
                Kind::SPAN,
            );
            Box::leak(Box::new(meta))
        })
    };
    // Empty value set — `skip_all` recorded no fields.
    let values = meta.fields().value_set(&[]);
    tracing::Span::new(meta, &values)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use mlua::{Lua, Table};
    use tracing::subscriber;
    use tracing_core::span::{Attributes, Id};
    use tracing_core::{Event, Subscriber};

    use super::{Module, luals_param_type, luals_returns, split_top_level};

    /// Minimal subscriber that records `(target, name)` of every span it is
    /// asked to create. Used to prove the facade opens an `#[instrument]`-
    /// shaped span around each call.
    #[derive(Default)]
    struct SpanRecorder {
        spans: Arc<Mutex<Vec<(String, String)>>>,
    }

    impl Subscriber for SpanRecorder {
        fn enabled(&self, _: &tracing_core::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, span: &Attributes<'_>) -> Id {
            let m = span.metadata();
            self.spans
                .lock()
                .unwrap()
                .push((m.target().to_string(), m.name().to_string()));
            Id::from_u64(1)
        }
        fn record(&self, _: &Id, _: &tracing_core::span::Record<'_>) {}
        fn record_follows_from(&self, _: &Id, _: &Id) {}
        fn event(&self, _: &Event<'_>) {}
        fn enter(&self, _: &Id) {}
        fn exit(&self, _: &Id) {}
    }

    /// A facade function opens a span named `"<stem>.<name>"` at target
    /// `"lua::lib::<module>"`, matching the old
    /// `#[instrument(name = …, target = …, skip_all)]` attribute.
    #[test]
    fn func_opens_instrument_shaped_span() {
        let recorder = SpanRecorder::default();
        let spans = recorder.spans.clone();

        let lua = Lua::new();
        let f: mlua::Function = subscriber::with_default(recorder, || {
            let mut m = Module::new(&lua, "lib:base64").expect("module");
            m.func("encode", &[("input", "string")], "doc", |_, s: String| {
                Ok(s)
            })
            .expect("func");
            m.into_table().get("encode").expect("get fn")
        });

        let _: String = f.call("hi").expect("call");
        let recorded = spans.lock().unwrap();
        assert_eq!(
            recorded.as_slice(),
            &[("lua::lib::base64".to_string(), "base64.encode".to_string(),)]
        );
    }

    /// A `std:*` registry key still traces under `lua::lib::<stem>`, matching
    /// the existing hand-written `#[instrument(target = …)]` convention.
    #[test]
    fn std_module_traces_under_lib_target() {
        let recorder = SpanRecorder::default();
        let spans = recorder.spans.clone();

        let lua = Lua::new();
        let f: mlua::Function = subscriber::with_default(recorder, || {
            let mut m = Module::new(&lua, "std:collections").expect("module");
            m.func("echo", &[("s", "string")], "doc", |_, s: String| Ok(s))
                .expect("func");
            m.into_table().get("echo").expect("get fn")
        });

        let _: String = f.call("hi").expect("call");
        let recorded = spans.lock().unwrap();
        assert_eq!(
            recorded.as_slice(),
            &[(
                "lua::lib::collections".to_string(),
                "collections.echo".to_string(),
            )]
        );
    }

    /// `func` / `try_func` accept any arity through `FromLuaMulti`: a
    /// two-argument function takes a tuple and records both params.
    #[test]
    fn multi_arg_function() {
        let lua = Lua::new();
        let mut m = Module::new(&lua, "vnd:demo").expect("module");
        m.try_func(
            "join",
            &[("a", "string"), ("b", "string")],
            "Concatenate two strings.",
            |_, (a, b): (String, String)| Ok::<_, String>(format!("{a}{b}")),
        )
        .expect("try_func");

        let meta = m.luals_meta().expect("meta");
        assert!(
            contains_in_order(
                &meta,
                &[
                    "--- Concatenate two strings.",
                    "---@param a string",
                    "---@param b string",
                    "---@return string?",
                    "---@return string? err",
                    "function M.join(a, b) end",
                ]
            ),
            "meta missing the multi-arg definition:\n{meta}"
        );

        let f: mlua::Function = m.into_table().get("join").expect("get fn");
        let (v, err): (String, Option<String>) = f.call(("foo", "bar")).expect("call");
        assert_eq!(v, "foobar");
        assert!(err.is_none());
    }

    /// The lines of `needle`, each found after the one before it.
    fn contains_in_order(haystack: &str, needles: &[&str]) -> bool {
        let mut rest = haystack;
        needles.iter().all(|n| match rest.find(n) {
            Some(at) => {
                rest = &rest[at + n.len()..];
                true
            }
            None => false,
        })
    }

    /// `userdata` records a class per type with its methods (`ud:method`, the implicit `self`), and the
    /// dynamic `LuaValue` bridge type surfaces as `any`.
    #[test]
    fn userdata_emits_class_and_methods() {
        let lua = Lua::new();
        let mut m = Module::new(&lua, "lib:json").expect("module");
        m.userdata("Object", |o| {
            o.method("get", &[("path", "LuaValue")], "LuaValue", "Read a path.")
                .method("len", &[], "number", "Number of keys.");
        });

        let meta = m.luals_meta().expect("meta");
        assert!(meta.starts_with("---@meta\n"), "{meta}");
        assert!(meta.contains("---@class lib_json.Object\n"), "{meta}");
        assert!(meta.contains("---@class lib_json\n"), "{meta}");
        assert!(
            contains_in_order(
                &meta,
                &[
                    "---@param path any",
                    "---@return any",
                    "function ud_Object:get(path) end"
                ]
            ),
            "{meta}"
        );
        assert!(
            contains_in_order(
                &meta,
                &["---@return number", "function ud_Object:len() end"]
            ),
            "{meta}"
        );
        assert!(meta.ends_with("---@type lib_json\nreturn M\n"), "{meta}");
    }

    /// `type_` registers a constructor on the type's sub-table (so
    /// `Type.new(...)` calls it) and records a `Type` class of methods and a
    /// `TypeStatics` class of constructors, reached from the module as a field.
    #[test]
    fn type_registers_and_records_constructor() {
        let lua = Lua::new();
        let table = {
            let mut m = Module::new(&lua, "std:collections").expect("module");
            m.type_("Map", |t| {
                t.constructor("new", &[], "Build an empty map.", |_, (): ()| Ok("MAP"))?
                    .method(
                        "set",
                        &[("k", "string"), ("v", "LuaValue")],
                        "()",
                        "Insert.",
                    );
                Ok(())
            })
            .expect("type_");

            let meta = m.luals_meta().expect("meta");
            assert!(meta.contains("---@class std_collections.Map\n"), "{meta}");
            assert!(
                meta.contains("---@field Map std_collections.MapStatics\n"),
                "{meta}"
            );
            assert!(meta.contains("function statics_Map.new() end"), "{meta}");
            assert!(
                contains_in_order(
                    &meta,
                    &[
                        "---@param k string",
                        "---@param v any",
                        "function ud_Map:set(k, v) end"
                    ]
                ),
                "{meta}"
            );
            m.into_table()
        };

        // The constructor is reachable as `Map.new()` and runs the body.
        let sub: Table = table.get("Map").expect("Map sub-table");
        let ctor: mlua::Function = sub.get("new").expect("new");
        let out: String = ctor.call(()).expect("call new");
        assert_eq!(out, "MAP");
    }

    /// `try_constructor` marshals the `(value, error)` idiom with a
    /// `"<Type>.<name>"` prefix and records a `Result<<Type>, string>` return.
    #[test]
    fn try_constructor_marshals_and_records() {
        let lua = Lua::new();
        let mut m = Module::new(&lua, "std:net").expect("module");
        m.type_("TcpListener", |t| {
            t.try_constructor(
                "bind",
                &[("addr", "string")],
                "Bind a listener.",
                |_, addr: String| {
                    if addr.is_empty() {
                        Err("empty addr".to_string())
                    } else {
                        Ok(addr)
                    }
                },
            )?;
            Ok(())
        })
        .expect("type_");

        let meta = m.luals_meta().expect("meta");
        assert!(
            contains_in_order(
                &meta,
                &[
                    "---@param addr string",
                    "---@return std_net.TcpListener?",
                    "---@return string? err",
                    "function statics_TcpListener.bind(addr) end",
                ]
            ),
            "{meta}"
        );

        let sub: Table = m.into_table().get("TcpListener").expect("sub");
        let bind: mlua::Function = sub.get("bind").expect("bind");
        let (_, err): (Option<String>, Option<String>) = bind.call("").expect("call");
        assert_eq!(err.as_deref(), Some("TcpListener.bind: empty addr"));
    }

    /// `const_` records a former module constant as a `---@field` on the module class, with an optional
    /// type spelled `name?`.
    #[test]
    fn const_records_field() {
        let lua = Lua::new();
        let mut m = Module::new(&lua, "std:thread").expect("module");
        m.const_("parent", "LuaValue", "Dynamic parent channel.");
        m.const_("shared", "LuaValue?", "Shared table.");

        let meta = m.luals_meta().expect("meta");
        assert!(
            meta.contains("---@field parent any Dynamic parent channel.\n"),
            "{meta}"
        );
        assert!(
            meta.contains("---@field shared? any Shared table.\n"),
            "{meta}"
        );
    }

    /// `try_async_func` records the same `(value, error)` returns as the synchronous `try_func`.
    #[test]
    fn try_async_func_records_result_signature() {
        let lua = Lua::new();
        let mut m = Module::new(&lua, "vnd:demo").expect("module");
        m.try_async_func(
            "open",
            &[("url", "string")],
            "Open a connection.",
            |_, url: String| async move { Ok::<_, String>(url) },
        )
        .expect("try_async_func");

        let meta = m.luals_meta().expect("meta");
        assert!(
            contains_in_order(
                &meta,
                &[
                    "---@param url string",
                    "---@return string?",
                    "---@return string? err",
                    "function M.open(url) end"
                ]
            ),
            "{meta}"
        );
    }

    /// `set` + `record_fn` registers a verbatim implementation and records its
    /// signature without the facade touching the body — the path for
    /// functions that raise or bake their own prefix.
    #[test]
    fn record_fn_records_signature_for_verbatim_set() {
        let lua = Lua::new();
        let mut m = Module::new(&lua, "lib:demo").expect("module");
        let raw = lua
            .create_function(|_, n: i64| Ok(n + 1))
            .expect("create_function");
        m.set("bump", raw).expect("set");
        m.record_fn("bump", &[("n", "number")], "number", "Increments by one.");

        let meta = m.luals_meta().expect("meta");
        assert!(
            contains_in_order(
                &meta,
                &[
                    "--- Increments by one.",
                    "---@param n number",
                    "---@return number",
                    "function M.bump(n) end"
                ]
            ),
            "{meta}"
        );

        // The verbatim implementation is what actually runs.
        let bump: mlua::Function = m.into_table().get("bump").expect("bump fn");
        let out: i64 = bump.call(41).expect("call");
        assert_eq!(out, 42);
    }

    /// A `()` return writes no `---@return`, a `Result<(), string>` returns `nil` then the error, an
    /// optional parameter is `name?`, and a Lua reserved word is made a safe name.
    #[test]
    fn return_and_name_shapes() {
        let lua = Lua::new();
        let mut m = Module::new(&lua, "lib:demo").expect("module");
        m.record_fn("noop", &[("end", "number?")], "()", "Does nothing.");
        m.record_fn("save", &[], "Result<(), string>", "Saves.");
        let meta = m.luals_meta().expect("meta");
        assert!(
            contains_in_order(
                &meta,
                &["---@param end_? number", "function M.noop(end_) end"]
            ),
            "{meta}"
        );
        let noop = &meta[meta.find("--- Does nothing.").expect("noop doc")..];
        let noop = &noop[..noop.find("function M.noop").expect("noop function")];
        assert!(
            !noop.contains("---@return"),
            "a `()` return wrote a return:\n{meta}"
        );
        assert!(
            contains_in_order(
                &meta,
                &[
                    "---@return nil",
                    "---@return string? err",
                    "function M.save() end"
                ]
            ),
            "{meta}"
        );
    }

    /// A tuple return writes one `---@return` per value, and a trailing `...` parameter is
    /// `---@param ... any` and a `...` in the signature.
    #[test]
    fn tuple_returns_and_variadic_params() {
        let lua = Lua::new();
        let mut m = Module::new(&lua, "lib:demo").expect("module");
        m.record_fn(
            "scan",
            &[("pattern", "string"), ("opts", "LuaValue?")],
            "(LuaValue, LuaValue?)",
            "Scans.",
        );
        m.record_fn(
            "call",
            &[("sql", "string"), ("...", "LuaValue")],
            "Result<number, string>",
            "Runs.",
        );
        let meta = m.luals_meta().expect("meta");
        assert!(
            contains_in_order(
                &meta,
                &[
                    "---@param pattern string",
                    "---@param opts? any",
                    "---@return any",
                    "---@return any?",
                    "function M.scan(pattern, opts) end",
                    "---@param sql string",
                    "---@param ... any",
                    "---@return number?",
                    "---@return string? err",
                    "function M.call(sql, ...) end",
                ]
            ),
            "{meta}"
        );
    }

    /// A userdata records fields (`---@field` on its class), dot-called functions (`.`, no receiver) and the
    /// statics of its sub-table on the module, and the sub-table is held to the record in both directions.
    #[test]
    fn userdata_fields_functions_and_statics() {
        let lua = Lua::new();
        let sub = lua.create_table().expect("table");
        sub.set("new", lua.create_function(|_, (): ()| Ok(1)).expect("fn"))
            .expect("set");
        sub.set("ZERO", 0).expect("set");
        let mut m = Module::new(&lua, "lib:demo").expect("module");
        m.set("Thing", sub.clone()).expect("set");
        m.userdata("Thing", |t| {
            t.field("size", "number", "How big.")
                .field("label", "string?", "A label.")
                .method("grow", &[("by", "number")], "()", "Grows.")
                .function("check", &[("expected", "LuaValue")], "()", "Checks.")
                .static_fn("new", &[], "Result<Thing, string>", "Builds one.")
                .static_const("ZERO", "Thing", "The empty one.");
        });

        let meta = m.luals_meta().expect("meta");
        assert!(meta.contains("---@field size number How big.\n"), "{meta}");
        assert!(
            meta.contains("---@field label? string A label.\n"),
            "{meta}"
        );
        assert!(meta.contains("function ud_Thing:grow(by) end"), "{meta}");
        assert!(
            meta.contains("function ud_Thing.check(expected) end"),
            "{meta}"
        );
        assert!(
            meta.contains("---@field ZERO lib_demo.Thing The empty one.\n"),
            "{meta}"
        );
        assert!(meta.contains("function statics_Thing.new() end"), "{meta}");
        assert!(
            meta.contains("---@field Thing lib_demo.ThingStatics\n"),
            "{meta}"
        );

        // a member of the sub-table that was not recorded is an error
        sub.set("extra", 1).expect("set");
        let error = m.luals_meta().expect_err("unrecorded member").to_string();
        assert!(
            error.contains("Thing.extra is on the module table but not recorded"),
            "{error}"
        );

        // and so is a recorded static that the sub-table does not have
        sub.set("extra", mlua::Value::Nil).expect("set");
        sub.set("ZERO", mlua::Value::Nil).expect("set");
        let error = m.luals_meta().expect_err("phantom static").to_string();
        assert!(
            error.contains("Thing.ZERO is recorded but the module table has no such member"),
            "{error}"
        );
    }

    /// A type that records statics but has no sub-table on the module is an error, not a silent omission.
    #[test]
    fn statics_without_a_sub_table_fail() {
        let lua = Lua::new();
        let mut m = Module::new(&lua, "lib:demo").expect("module");
        m.userdata("Thing", |t| {
            t.static_fn("new", &[], "Thing", "Builds one.");
        });
        let error = m.luals_meta().expect_err("no sub-table").to_string();
        assert!(
            error.contains("records statics but the module has no such sub-table"),
            "{error}"
        );
    }

    /// Reserved words are escaped in function, parameter and field names (including `goto`, reserved in Lua 5.2+), and a
    /// near miss is left alone.
    #[test]
    fn reserved_words_are_escaped_everywhere() {
        let lua = Lua::new();
        let mut m = Module::new(&lua, "lib:demo").expect("module");
        m.record_fn(
            "goto",
            &[("local", "string"), ("ending", "string")],
            "()",
            "Jumps.",
        );
        m.const_("end", "number", "A constant.");
        m.userdata("Thing", |t| {
            t.field("repeat", "number", "A field.").method(
                "then",
                &[("until", "number")],
                "()",
                "A method.",
            );
        });
        let meta = m.luals_meta().expect("meta");
        assert!(
            meta.contains("function M.goto_(local_, ending) end"),
            "{meta}"
        );
        assert!(
            meta.contains("---@field end_ number A constant.\n"),
            "{meta}"
        );
        assert!(
            meta.contains("---@field repeat_ number A field.\n"),
            "{meta}"
        );
        assert!(
            meta.contains("function ud_Thing:then_(until_) end"),
            "{meta}"
        );
        // the output is still valid Lua (it loads on the engine, Luau)
        lua.load(meta.as_str()).into_function().expect("valid Lua");
    }

    /// `Result<T, string>` makes `T` optional unless it already is (or is `any`), a `()` value is `nil`,
    /// and `boolean` is the Lua name of `bool`.
    #[test]
    fn result_value_shapes() {
        let lua = Lua::new();
        let mut m = Module::new(&lua, "lib:demo").expect("module");
        m.record_fn("a", &[], "Result<LuaValue, string>", "A.");
        m.record_fn("b", &[], "Result<string?, string>", "B.");
        m.record_fn("c", &[("flag", "bool")], "Result<bool, string>", "C.");
        let meta = m.luals_meta().expect("meta");
        assert!(
            contains_in_order(
                &meta,
                &[
                    "---@return any\n",
                    "---@return string? err",
                    "function M.a() end"
                ]
            ),
            "{meta}"
        );
        assert!(
            contains_in_order(
                &meta,
                &[
                    "---@return string?\n",
                    "---@return string? err",
                    "function M.b() end"
                ]
            ),
            "{meta}"
        );
        assert!(
            contains_in_order(
                &meta,
                &[
                    "---@param flag boolean",
                    "---@return boolean?\n",
                    "---@return string? err",
                    "function M.c(flag) end"
                ]
            ),
            "{meta}"
        );
    }

    /// An empty doc writes no comment line and a multi-line doc is flattened into a single `---@field` line.
    #[test]
    fn docs_empty_and_multiline() {
        let lua = Lua::new();
        let mut m = Module::new(&lua, "lib:demo").expect("module");
        m.record_fn("quiet", &[], "()", "");
        m.const_("noisy", "number", "first line\nsecond line");
        let meta = m.luals_meta().expect("meta");
        assert!(
            meta.contains("local M = {}\n\nfunction M.quiet() end"),
            "{meta}"
        );
        assert!(
            meta.contains("---@field noisy number first line second line\n"),
            "{meta}"
        );
    }

    /// A type that records only constants still gets a statics class and a module field, and a recorded
    /// static function the sub-table lacks is a phantom, like a constant.
    #[test]
    fn const_only_statics_and_phantom_static_fn() {
        let lua = Lua::new();
        let sub = lua.create_table().expect("table");
        sub.set("MAX", 1).expect("set");
        let mut m = Module::new(&lua, "lib:demo").expect("module");
        m.set("Limit", sub.clone()).expect("set");
        m.userdata("Limit", |t| {
            t.static_const("MAX", "number", "The cap.");
        });
        let meta = m.luals_meta().expect("meta");
        assert!(
            meta.contains("---@field Limit lib_demo.LimitStatics\n"),
            "{meta}"
        );
        assert!(meta.contains("---@field MAX number The cap.\n"), "{meta}");

        let mut m = Module::new(&lua, "lib:demo").expect("module");
        m.set("Limit", sub).expect("set");
        m.userdata("Limit", |t| {
            t.static_const("MAX", "number", "The cap.").static_fn(
                "make",
                &[],
                "Limit",
                "Makes one.",
            );
        });
        let error = m.luals_meta().expect_err("phantom static fn").to_string();
        assert!(
            error.contains("Limit.make is recorded but the module table has no such member"),
            "{error}"
        );
    }

    /// A trailing `?` on a `fun(...)` type belongs to its return type, so the parameter is not optional;
    /// `(fun(...): T)?` is the optional function; a class of another module passes through and may be optional.
    #[test]
    fn pass_through_types_and_optionals() {
        assert_eq!(
            luals_param_type("fun(a, b): string?", "lib_x"),
            ("fun(a, b): string?".to_string(), false)
        );
        assert_eq!(
            luals_param_type("(fun(a): string)?", "lib_x"),
            ("(fun(a): string)".to_string(), true)
        );
        assert_eq!(
            luals_param_type("std_net.TcpListener?", "lib_x"),
            ("std_net.TcpListener".to_string(), true)
        );
        assert_eq!(
            luals_param_type("std_net.TcpListener", "lib_x"),
            ("std_net.TcpListener".to_string(), false)
        );
        assert_eq!(
            luals_param_type("Thing?", "lib_x"),
            ("lib_x.Thing".to_string(), true)
        );

        let lua = Lua::new();
        let mut m = Module::new(&lua, "lib:demo").expect("module");
        m.record_fn(
            "serve",
            &[
                ("listener", "std_net.TcpListener"),
                ("handler", "fun(req: vnd_hyper.Request): string|nil"),
                ("on_error", "(fun(message: string): nil)?"),
            ],
            "()",
            "Serves.",
        );
        m.const_("hook", "fun(a, b): string?", "A hook.");
        let meta = m.luals_meta().expect("meta");
        assert!(
            contains_in_order(
                &meta,
                &[
                    "---@param listener std_net.TcpListener\n",
                    "---@param handler fun(req: vnd_hyper.Request): string|nil\n",
                    "---@param on_error? (fun(message: string): nil)\n",
                    "function M.serve(listener, handler, on_error) end",
                ]
            ),
            "{meta}"
        );
        assert!(
            meta.contains("---@field hook fun(a, b): string? A hook.\n"),
            "{meta}"
        );
    }

    /// A tuple is split on its top-level commas only, so a `fun(a, b)` inside it stays whole, and a `...`
    /// anywhere in it, or alone, is `any ...`.
    #[test]
    fn tuple_returns_split_on_top_level_commas() {
        assert_eq!(
            split_top_level("string, fun(a, b): c, table<string, any>"),
            ["string", " fun(a, b): c", " table<string, any>"]
        );
        assert_eq!(
            luals_returns("(fun(a, b): string, ())", "lib_x"),
            ["fun(a, b): string", "nil"]
        );
        assert_eq!(
            luals_returns("(LuaValue?, ...)", "lib_x"),
            ["any?", "any ..."]
        );
        assert_eq!(luals_returns("...", "lib_x"), ["any ..."]);
        assert_eq!(luals_returns("(string, ())", "lib_x"), ["string", "nil"]);
        assert_eq!(luals_returns("()", "lib_x"), Vec::<String>::new());
    }
}
