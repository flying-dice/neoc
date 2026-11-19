//! A facade over mlua module construction that also records each module's surface.
//!
//! A module is declared once, and that single declaration both registers functions with mlua exactly
//! as a hand-written `module()` would and records the signature + doc for each function (and, for
//! resource handles, each method). The recording is dumped as a declaration header via
//! [`Module::decl_header`]. There is no reverse mapping: the Rust source is the source of truth.
//!
//! The header syntax (`extern { mod ... }`, `Result<T, E>`, `Option<T>`) is the declaration dialect of
//! `neoc`, where this VM was written, and the comments below call its types "neoc types". Nothing in this
//! repo reads the headers yet (the generated Lua typings come from the Rust schema), so they exist
//! for the module-surface tests in `modules.rs`.
//!
//! Three things the facade preserves byte-for-byte versus the hand-written
//! `module()` functions it replaces:
//!
//! 1. **The `(value, error)` shape.** [`Module::try_func`] routes through
//!    [`crate::lua::sandbox::ok`] / [`crate::lua::sandbox::from_err`], so a
//!    fallible function returns `(v, nil)` on success and
//!    `(nil, "<stem>.<fn>: <e>")` on failure — the identical idiom and the
//!    identical error prefix the original used.
//! 2. **Per-call tracing spans.** Every registered function opens a
//!    `tracing` span named `"<stem>.<name>"` at target
//!    `"lua::lib::<module>"` around the call body, matching the old
//!    `#[instrument(name = …, target = …, skip_all)]` attribute. See
//!    [`dyn_span`].
//! 3. **The registered Lua surface.** The functions and userdata types land
//!    on the table exactly as `lua.create_function` / `lua.create_userdata`
//!    would place them.

use std::collections::HashMap;
use std::future::Future;
use std::sync::OnceLock;

use parking_lot::Mutex;

use mlua::{FromLuaMulti, IntoLua, IntoLuaMulti, Lua, Table};
use tracing::{Instrument, Level};
use tracing_core::callsite::{Callsite, DefaultCallsite, Identifier};
use tracing_core::field::FieldSet;
use tracing_core::metadata::{Kind, Metadata};

use crate::lua::sandbox::{from_err, ok};

/// Maps a Rust binding type to its neoc type name.
///
/// Implemented for the bridge types a Lua-bound function takes or returns.
/// The associated name is the neoc surface type, not the Rust type: every
/// string-shaped binding (`mlua::LuaString`, `String`, `&str`, `Vec<u8>`)
/// collapses to `"string"`, every numeric binding collapses to `"number"`,
/// and every dynamic value binding (`mlua::Value`, `mlua::MultiValue`,
/// `mlua::AnyUserData`) collapses to the opaque `"LuaValue"`.
pub trait DeclType {
    /// Returns the neoc type name for this binding type.
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

/// One recorded free function: its name, parameter list (name + neoc type
/// per param), return type, and doc string. Enough to emit one `fn` line +
/// its `///` doc.
struct Entry {
    name: String,
    params: Vec<(String, String)>,
    ret: String,
    doc: String,
}

impl Entry {
    /// Emits a bodyless `fn` line.
    ///
    /// `receiver` is the leading `self` parameter when the entry is an instance
    /// method (prepended ahead of the recorded params), or `None` for a free or
    /// associated function. A `()` return is elided to a bare signature, since
    /// the extern grammar treats the return clause as optional.
    fn signature(&self, receiver: Option<&str>) -> String {
        let mut parts = Vec::new();
        if let Some(r) = receiver {
            parts.push(r.to_string());
        }
        parts.extend(self.params.iter().map(|(n, t)| format!("{n}: {}", decl_ty(t))));
        let params = parts.join(", ");
        let ret = decl_ty(&self.ret);
        if ret == "()" {
            format!("fn {}({params});", self.name)
        } else {
            format!("fn {}({params}) -> {ret};", self.name)
        }
    }

    /// Emits a constructor line: `#[extern(constructor)] fn <name>(<params>);`.
    /// A constructor has no return clause — it constructs the enclosing `Self`.
    fn constructor_signature(&self) -> String {
        let params =
            self.params.iter().map(|(n, t)| format!("{n}: {}", decl_ty(t))).collect::<Vec<_>>().join(", ");
        format!("#[extern(constructor)] fn {}({params});", self.name)
    }
}

/// Translates a recorded neoc type string to its emitted surface form.
///
/// Two normalisations:
///
/// 1. The facade records the opaque dynamic type as `LuaValue` (its bridge
///    name), but the extern surface speaks the language primitive `Unknown`.
/// 2. The facade records optionality with a trailing `?` (`string?`), but neoc
///    has no `?` sugar — optionality is the nominal `Option<T>`. A trailing `?`
///    becomes `Option<…>` around the (recursively translated) base type.
///
/// Every other type passes through unchanged.
fn decl_ty(ty: &str) -> String {
    let ty = ty.trim();
    if let Some(base) = ty.strip_suffix('?') {
        return format!("Option<{}>", decl_ty(base));
    }
    ty.replace("LuaValue", "Unknown")
}

/// One recorded `struct` + `impl` for a resource-handle userdata type. The
/// receiver (`self`) is implicit on every method, so only the trailing
/// arguments are recorded.
struct UserDataEntry {
    type_name: String,
    methods: Vec<Entry>,
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

/// One recorded module-level constant: a value bound directly on the module
/// table whose shape the facade does not model as a function (for example
/// `std:thread`'s dynamically-injected `parent` / `shared`). The extern item
/// set has no `const`, so it is emitted inside the `mod` as a zero-argument
/// accessor `fn <name>() -> <type>;`.
struct ConstEntry {
    name: String,
    ty: String,
    doc: String,
}

/// Builds a Lua module table while recording the neoc signature of each
/// function and userdata type it registers.
///
/// Register free functions with [`Module::func`] (infallible) and
/// [`Module::try_func`] (fallible); both accept any arity through
/// `FromLuaMulti` plus an explicit parameter-name/type slice. Record a
/// resource-handle userdata type with [`Module::userdata`]. When the surface
/// is complete, hand the table to the engine with [`Module::into_table`] and
/// emit the matching neoc header with [`Module::decl_header`].
pub struct Module<'l> {
    lua: &'l Lua,
    table: Table,
    name: String,
    entries: Vec<Entry>,
    userdata: Vec<UserDataEntry>,
    types: Vec<TypeEntry>,
    consts: Vec<ConstEntry>,
}

impl<'l> Module<'l> {
    /// Creates a new module builder with an empty backing table.
    ///
    /// The `name` is the registry key the module is installed under
    /// (for example, `"lib:base64"`) and appears in the emitted header.
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
    /// exactly as the old `#[instrument]` attribute did. The recorded neoc
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
        let span = dyn_span(&self.target(), &format!("{}.{}", module_stem(&self.name), name));
        self.table.set(name, self.lua.create_function(move |lua, a: A| span.in_scope(|| f(lua, a)))?)?;
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
    /// the function is wrapped in `lua.create_async_function` so calls yield
    /// the calling Lua coroutine until the future resolves. The future runs
    /// inside the same `"<stem>.<name>"` tracing span [`Module::func`] uses,
    /// attached via [`tracing::Instrument`] so the span covers the whole
    /// `await`. neoc has no async surface — async is a runtime detail — so the
    /// recorded neoc signature is identical to a synchronous `fn`:
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
        R: DeclType + IntoLuaMulti + Send,
        Fut: Future<Output = mlua::Result<R>> + Send + 'static,
    {
        let span = dyn_span(&self.target(), &format!("{}.{}", module_stem(&self.name), name));
        let func = self.lua.create_async_function(move |lua, a: A| {
            let fut = f(lua, a);
            let span = span.clone();
            async move { crate::lua::lifecycle::idle_result(fut.instrument(span)).await }
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
    /// The recorded neoc return type is `Result<<T>, string>`, identical to
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
        T: DeclType + IntoLua + Send,
        E: std::fmt::Display + Send,
        Fut: Future<Output = Result<T, E>> + Send + 'static,
    {
        let prefix = format!("{}.{}", module_stem(&self.name), name);
        let span = dyn_span(&self.target(), &prefix);
        let func = self.lua.create_async_function(move |lua, a: A| {
            let fut = f(lua.clone(), a);
            let span = span.clone();
            let prefix = prefix.clone();
            async move {
                match crate::lua::lifecycle::idle(fut.instrument(span)).await? {
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

    /// Records a free-function signature for the header without registering an
    /// implementation.
    ///
    /// The signature-only counterpart to [`Module::set`]: pair the two when a
    /// free function must be registered verbatim — because it raises an
    /// `mlua::Error` directly, bakes its own error prefix, shares a prefix
    /// helper with a userdata method, or runs under a custom or absent
    /// `tracing` span — yet must still appear in the emitted header. `params`
    /// is the ordered list of `(parameter_name, decl_type)` pairs; `ret` is the
    /// neoc return type as a string (the caller passes it directly because such
    /// a function's return shape is decided in its body rather than by a single
    /// Rust return type, exactly as [`UserDataSpec::method`] does). This records
    /// only the header surface; the runtime registration is whatever
    /// [`Module::set`] installed, left untouched.
    pub fn record_fn(&mut self, name: &str, params: &[(&str, &str)], ret: &str, doc: &str) -> &mut Self {
        self.entries.push(Entry {
            name: name.to_string(),
            params: own_params(params),
            ret: ret.to_string(),
            doc: doc.to_string(),
        });
        self
    }

    /// Records a resource-handle userdata type for the emitted header.
    ///
    /// `type_name` is the neoc type the userdata maps to (matching the Rust
    /// type name, e.g. `"Object"`). The closure receives a [`UserDataSpec`]
    /// recorder; declare each method on it with [`UserDataSpec::method`]. This
    /// only records the neoc surface — the runtime registration of the type's
    /// methods stays in the type's own `impl UserData`, untouched, so its
    /// behaviour is byte-identical to today.
    pub fn userdata(&mut self, type_name: &str, build: impl FnOnce(&mut UserDataSpec)) -> &mut Self {
        let mut spec = UserDataSpec { type_name: type_name.to_string(), methods: Vec::new() };
        build(&mut spec);
        self.userdata.push(UserDataEntry { type_name: spec.type_name, methods: spec.methods });
        self
    }

    /// Registers a constructor-bearing type exposed as a sub-table.
    ///
    /// Models the pattern where a module surfaces a type through a sub-table
    /// holding one or more constructors — `std:collections`'s
    /// `Map.new()` / `Counter.new(initial)`, `std:net`'s
    /// `TcpListener.bind(addr)`. The closure receives a [`TypeSpec`]: declare
    /// each constructor with [`TypeSpec::constructor`] (it both registers the
    /// function on the sub-table with mlua and records an associated `fn`) and
    /// each instance method with [`TypeSpec::method`] (recorded for the header
    /// only — the method's runtime registration stays in the type's own
    /// `impl UserData`). The sub-table is created here, populated with the
    /// constructors, and set on the module table under `type_name`, exactly as
    /// a hand-written `t.set("Map", map_t)` would. The recorded header is
    /// `struct <Type>; impl <Type> { <constructors> <methods> }`.
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
        let entry =
            TypeEntry { type_name: spec.type_name, constructors: spec.constructors, methods: spec.methods };
        self.table.set(type_name, sub)?;
        self.types.push(entry);
        Ok(self)
    }

    /// Records a module-level constant for the emitted header.
    ///
    /// For a value bound directly on the module table whose shape the facade
    /// does not model as a function — most notably a dynamically-injected
    /// global such as `std:thread`'s `parent` / `shared`, which only exists in
    /// a spawned worker and cannot be typed beyond the opaque `Unknown`. This
    /// only records the neoc surface (a zero-argument accessor
    /// `fn <name>() -> <type>;` inside the `mod`, since the extern item set has
    /// no `const`); the runtime value is set elsewhere (or injected at spawn
    /// time), so the registration is unchanged.
    pub fn const_(&mut self, name: &str, ty: &str, doc: &str) -> &mut Self {
        self.consts.push(ConstEntry { name: name.to_string(), ty: ty.to_string(), doc: doc.to_string() });
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

    /// Emits the declaration header text for this module.
    ///
    /// The output is one `extern { … }` block wrapping a single
    /// `#[extern(path = "<key>")] mod <name> { … }`, where `<key>` is the exact
    /// registry key (`require()` path) and `<name>` is the neoc identifier
    /// derived from the key's last segment. The module's whole surface lives
    /// inside the `mod`: each resource-handle type as a `struct` + bodyless
    /// `impl`, each free function as a bodyless `fn`, and each former module
    /// constant as a zero-argument accessor `fn`. Every nested item is indented
    /// one level deeper than its enclosing block, and the output is terminated
    /// by a single trailing newline.
    // TODO: clean-code - 0.5 - DEAD: the signature and doc recording behind `decl_header`/`headers` has no production reader (only tests); keep just the span and registration half.
    pub fn decl_header(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("// @generated from the {} runtime module — do not edit.\n", self.name));
        out.push_str("extern {\n");
        out.push_str(&format!("    #[extern(path = \"{}\")]\n", self.name));
        out.push_str(&format!("    mod {} {{\n", mod_name(&self.name)));

        // Resource-handle types: every method takes the implicit `self`.
        for ud in &self.userdata {
            out.push_str(&format!("        struct {};\n", ud.type_name));
            out.push_str(&format!("        impl {} {{\n", ud.type_name));
            for m in &ud.methods {
                out.push_str(&format!("            /// {}\n", m.doc));
                out.push_str(&format!("            {}\n", m.signature(Some("self"))));
            }
            out.push_str("        }\n");
        }

        // Constructor-bearing types: constructors are associated (no `self`,
        // marked `#[extern(constructor)]`); instance methods take `self`.
        for ty in &self.types {
            out.push_str(&format!("        struct {};\n", ty.type_name));
            out.push_str(&format!("        impl {} {{\n", ty.type_name));
            for c in &ty.constructors {
                out.push_str(&format!("            /// {}\n", c.doc));
                out.push_str(&format!("            {}\n", c.constructor_signature()));
            }
            for m in &ty.methods {
                out.push_str(&format!("            /// {}\n", m.doc));
                out.push_str(&format!("            {}\n", m.signature(Some("self"))));
            }
            out.push_str("        }\n");
        }

        // Free functions.
        for e in &self.entries {
            out.push_str(&format!("        /// {}\n", e.doc));
            out.push_str(&format!("        {}\n", e.signature(None)));
        }

        // Former module constants: a value bound on the module table, modelled
        // as a zero-argument accessor `fn` (the extern item set has no `const`).
        for c in &self.consts {
            out.push_str(&format!("        /// {}\n", c.doc));
            out.push_str(&format!("        fn {}() -> {};\n", c.name, decl_ty(&c.ty)));
        }

        out.push_str("    }\n");
        out.push_str("}\n");
        out
    }
}

/// Records the neoc method surface of one resource-handle userdata type.
///
/// Handed to the closure passed to [`Module::userdata`]. Each
/// [`UserDataSpec::method`] call mirrors one `add_method` / `add_function` on
/// the type's `impl UserData`. The receiver (`self`) is implicit and is not
/// listed in `params`.
pub struct UserDataSpec {
    type_name: String,
    methods: Vec<Entry>,
}

impl UserDataSpec {
    /// Records one method on the userdata type.
    ///
    /// `params` is the ordered list of `(parameter_name, decl_type)` pairs for
    /// the arguments *after* the implicit `self` receiver. `ret` is the neoc
    /// return type as a string (callers pass it directly because a method's
    /// return shape — often `LuaValue` or `(LuaValue, LuaValue)` — is decided
    /// at the `add_method` body rather than by a single Rust return type).
    pub fn method(&mut self, name: &str, params: &[(&str, &str)], ret: &str, doc: &str) -> &mut Self {
        self.methods.push(Entry {
            name: name.to_string(),
            params: own_params(params),
            ret: ret.to_string(),
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
/// `fn`s with no `self`. Methods are recorded for the header only — their
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
        self.sub.set(name, self.lua.create_function(move |lua, a: A| span.in_scope(|| f(lua, a)))?)?;
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
    /// the implicit `self` receiver, and `ret` is the neoc return type as a
    /// string. This only records the header surface — the method's runtime
    /// registration lives in the type's own `impl UserData`.
    pub fn method(&mut self, name: &str, params: &[(&str, &str)], ret: &str, doc: &str) -> &mut Self {
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
    params.iter().map(|(n, t)| (n.to_string(), t.to_string())).collect()
}

/// Strips the namespace prefix from a registry key for use in error prefixes
/// and span names: `"lib:base64"` becomes `"base64"`. The namespace is a
/// registration concern, not part of the runtime error identity (see the
/// style guide).
fn module_stem(name: &str) -> &str {
    name.split_once(':').map_or(name, |(_, stem)| stem)
}

/// Derives the `mod` identifier emitted in a header from a registry key: the
/// key's last segment (`"vnd:rusqlite"` → `"rusqlite"`, `"std:fs"` → `"fs"`).
/// This is what makes the backend emit `local <name> = require("<key>")` and
/// lower member access to `<name>.member`. Runtime keys already use
/// identifier-safe last segments (letters, digits, underscores), so the segment
/// is returned verbatim.
fn mod_name(name: &str) -> &str {
    module_stem(name)
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
        "agentos_luavm.facade",
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
    static CACHE: OnceLock<Mutex<HashMap<(String, String), &'static Metadata<'static>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Builds (or fetches the interned) `INFO`-level span `Metadata` for the
/// given target + name, then returns an unentered [`tracing::Span`] over it.
/// Entering the returned span around a call body reproduces the old
/// `#[instrument]` span: same name, same `lua::…` target, no fields.
fn dyn_span(target: &str, name: &str) -> tracing::Span {
    let key = (target.to_string(), name.to_string());
    let meta = {
        let mut cache = metadata_cache().lock();
        *cache.entry(key).or_insert_with(|| {
            let name: &'static str = Box::leak(name.to_string().into_boxed_str());
            let target: &'static str = Box::leak(target.to_string().into_boxed_str());
            let fields = FieldSet::new(&[], shared_callsite().metadata().callsite());
            let meta = Metadata::new(name, target, Level::INFO, None, None, None, fields, Kind::SPAN);
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

    use super::Module;

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
            self.spans.lock().unwrap().push((m.target().to_string(), m.name().to_string()));
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
            m.func("encode", &[("input", "string")], "doc", |_, s: String| Ok(s)).expect("func");
            m.into_table().get("encode").expect("get fn")
        });

        let _: String = f.call("hi").expect("call");
        let recorded = spans.lock().unwrap();
        assert_eq!(recorded.as_slice(), &[("lua::lib::base64".to_string(), "base64.encode".to_string(),)]);
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
            m.func("echo", &[("s", "string")], "doc", |_, s: String| Ok(s)).expect("func");
            m.into_table().get("echo").expect("get fn")
        });

        let _: String = f.call("hi").expect("call");
        let recorded = spans.lock().unwrap();
        assert_eq!(
            recorded.as_slice(),
            &[("lua::lib::collections".to_string(), "collections.echo".to_string(),)]
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

        let header = m.decl_header();
        assert!(
            header.contains("fn join(a: string, b: string) -> Result<string, string>;"),
            "header missing multi-arg signature:\n{header}"
        );

        let f: mlua::Function = m.into_table().get("join").expect("get fn");
        let (v, err): (String, Option<String>) = f.call(("foo", "bar")).expect("call");
        assert_eq!(v, "foobar");
        assert!(err.is_none());
    }

    /// `userdata` records a `struct` + `impl` block inside the `mod`; each
    /// method takes an implicit `self`, and the dynamic `LuaValue` bridge type
    /// surfaces as the `Unknown` primitive.
    #[test]
    fn userdata_emits_struct_impl() {
        let lua = Lua::new();
        let mut m = Module::new(&lua, "lib:json").expect("module");
        m.userdata("Object", |o| {
            o.method("get", &[("path", "LuaValue")], "LuaValue", "Read a path.").method(
                "len",
                &[],
                "number",
                "Number of keys.",
            );
        });

        let header = m.decl_header();
        assert!(header.contains("extern {"), "{header}");
        assert!(header.contains("#[extern(path = \"lib:json\")]"), "{header}");
        assert!(header.contains("mod json {"), "{header}");
        assert!(header.contains("struct Object;"), "{header}");
        assert!(header.contains("impl Object {"), "{header}");
        assert!(header.contains("fn get(self, path: Unknown) -> Unknown;"), "{header}");
        assert!(header.contains("fn len(self) -> number;"), "{header}");
    }

    /// `type_` registers a constructor on the type's sub-table (so
    /// `Type.new(...)` calls it) and records `struct … impl … { fn new() …
    /// <methods> }`.
    #[test]
    fn type_registers_and_records_constructor() {
        let lua = Lua::new();
        let table = {
            let mut m = Module::new(&lua, "std:collections").expect("module");
            m.type_("Map", |t| {
                t.constructor("new", &[], "Build an empty map.", |_, (): ()| Ok("MAP"))?.method(
                    "set",
                    &[("k", "string"), ("v", "LuaValue")],
                    "()",
                    "Insert.",
                );
                Ok(())
            })
            .expect("type_");

            let header = m.decl_header();
            assert!(header.contains("struct Map;"), "{header}");
            assert!(header.contains("#[extern(constructor)] fn new();"), "{header}");
            assert!(header.contains("fn set(self, k: string, v: Unknown);"), "{header}");
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
            t.try_constructor("bind", &[("addr", "string")], "Bind a listener.", |_, addr: String| {
                if addr.is_empty() {
                    Err("empty addr".to_string())
                } else {
                    Ok(addr)
                }
            })?;
            Ok(())
        })
        .expect("type_");

        let header = m.decl_header();
        assert!(header.contains("#[extern(constructor)] fn bind(addr: string);"), "{header}");

        let sub: Table = m.into_table().get("TcpListener").expect("sub");
        let bind: mlua::Function = sub.get("bind").expect("bind");
        let (_, err): (Option<String>, Option<String>) = bind.call("").expect("call");
        assert_eq!(err.as_deref(), Some("TcpListener.bind: empty addr"));
    }

    /// `const_` records a former module constant as a zero-argument accessor
    /// `fn` inside the `mod`, with `LuaValue` surfaced as `Unknown` (the extern
    /// item set has no `const`).
    #[test]
    fn const_records_accessor_fn() {
        let lua = Lua::new();
        let mut m = Module::new(&lua, "std:thread").expect("module");
        m.const_("parent", "LuaValue", "Dynamic parent channel.");

        let header = m.decl_header();
        assert!(header.contains("fn parent() -> Unknown;"), "{header}");
    }

    /// `try_async_func` records the same `Result<<T>, string>` signature as
    /// the synchronous `try_func`.
    #[test]
    fn try_async_func_records_result_signature() {
        let lua = Lua::new();
        let mut m = Module::new(&lua, "vnd:demo").expect("module");
        m.try_async_func("open", &[("url", "string")], "Open a connection.", |_, url: String| async move {
            Ok::<_, String>(url)
        })
        .expect("try_async_func");

        let header = m.decl_header();
        assert!(header.contains("fn open(url: string) -> Result<string, string>;"), "{header}");
    }

    /// `set` + `record_fn` registers a verbatim implementation and records its
    /// header signature without the facade touching the body — the path for
    /// functions that raise or bake their own prefix.
    #[test]
    fn record_fn_records_signature_for_verbatim_set() {
        let lua = Lua::new();
        let mut m = Module::new(&lua, "lib:demo").expect("module");
        let raw = lua.create_function(|_, n: i64| Ok(n + 1)).expect("create_function");
        m.set("bump", raw).expect("set");
        m.record_fn("bump", &[("n", "number")], "number", "Increments by one.");

        let header = m.decl_header();
        assert!(header.contains("    fn bump(n: number) -> number;"), "{header}");

        // The verbatim implementation is what actually runs.
        let bump: mlua::Function = m.into_table().get("bump").expect("bump fn");
        let out: i64 = bump.call(41).expect("call");
        assert_eq!(out, 42);
    }
}
