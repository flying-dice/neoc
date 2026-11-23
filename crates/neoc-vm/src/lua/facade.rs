//! A declarative facade over mlua module construction.
//!
//! A module is declared once, and that single declaration both registers
//! functions with mlua exactly as a hand-written `module()` would and records
//! the signature + doc for each function (and, for resource handles, each
//! method, field and constructor). The recording is dumped as Luau
//! declarations by [`Module::luau_decls`] (the `luau_decls` child module):
//! `cargo run -p neoc-vm --example luau_decls` writes every module's to
//! `types/neoc.d.luau`, and luau-lsp makes `require("lib:json")` resolve to
//! its type, with completion and diagnostics. There is no reverse
//! mapping: the Rust source is the source of truth.
//!
//! A module does not write a signature beside its implementation. The attributes of `neoc_macros` (`lua_fn`,
//! `lua_userdata`, `lua_members`, `lua_module`) read the name, the parameters and the doc from the Rust and
//! call the recording primitives here (`record_fn`, `const_`, `type_const`, `UserDataSpec::method`,
//! `static_fn` and the rest). Those primitives stay public to the crate for the generated code and for this
//! file's own tests; a module that calls one by hand is declaring something twice.
//!
//! The types say as much as they can. A function that can fail returns a
//! [`Fallible`](crate::lua::sandbox::Fallible), a userdata type says its name ([`Named`]), a userdata argument
//! is a [`Handle`], a list is a [`List`], and what a closure returns is read from its type by the functions of
//! [`typed`]. A string beside the code is for what Rust cannot say: a `Table` or a `Value` that a script
//! knows as something narrower.
//!
//! Three things the facade preserves byte-for-byte versus the hand-written
//! `module()` functions it replaces:
//!
//! 1. **The `(value, error)` shape.** [`Module::try_func`] routes through
//!    [`crate::lua::sandbox::ok`] / [`crate::lua::sandbox::from_err`], so a
//!    fallible function returns `(v, nil)` on success and
//!    `(nil, "<stem>.<fn>: <e>")` on failure — the identical idiom and the
//!    identical error prefix the original used. The declarations say the
//!    same: the return type `(T?, string?)`.
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

pub mod luau_decls;
#[cfg(test)]
mod macros_tests;
pub mod typed;

use crate::lua::lifecycle::{Role, async_function};
use crate::lua::sandbox::{Fallible, from_err, ok};

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

/// A userdata type that says the module it is a type of and its name there
/// (`#[neoc_macros::lua_userdata("std:time", "Duration")]`), so that a function that takes or returns it
/// declares it by its Rust type.
pub trait Named {
    /// The key of the module the type is declared by (`std:time`).
    const MODULE: &'static str;
    /// The name of the type in that module (`Duration`).
    const NAME: &'static str;
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

impl<T: DeclType> DeclType for Fallible<T> {
    fn decl_type() -> String {
        format!("Result<{}, string>", T::decl_type())
    }
}

/// A userdata argument that is declared as the type `T` it is meant to be: `duration: Handle<DurationHandle>`
/// is `duration: Duration` to a script. It is the userdata as it was passed, so the function decides what a
/// wrong one is (an error it returns, or one it raises): `duration.borrow::<DurationHandle>()`.
pub struct Handle<T>(mlua::AnyUserData, std::marker::PhantomData<fn() -> T>);

impl<T> std::ops::Deref for Handle<T> {
    type Target = mlua::AnyUserData;

    fn deref(&self) -> &mlua::AnyUserData {
        &self.0
    }
}

impl<T> mlua::FromLua for Handle<T> {
    fn from_lua(value: mlua::Value, lua: &Lua) -> mlua::Result<Self> {
        Ok(Self(
            mlua::AnyUserData::from_lua(value, lua)?,
            std::marker::PhantomData,
        ))
    }
}

impl<T: DeclType> DeclType for Handle<T> {
    fn decl_type() -> String {
        T::decl_type()
    }
}

/// A list a function hands a script: a table of `T` in order, declared `{ T }`. (A bare `Vec<u8>` is the bytes
/// of a string, so a list says that it is one.)
pub struct List<T>(pub Vec<T>);

impl<T: IntoLua> IntoLua for List<T> {
    fn into_lua(self, lua: &Lua) -> mlua::Result<mlua::Value> {
        self.0.into_lua(lua)
    }
}

impl<T: DeclType> DeclType for List<T> {
    fn decl_type() -> String {
        format!("{{ {} }}", T::decl_type())
    }
}

impl<T: DeclType, E: DeclType> DeclType for Result<T, E> {
    fn decl_type() -> String {
        format!("Result<{}, {}>", T::decl_type(), E::decl_type())
    }
}

/// One recorded function: its name, parameter list (name + recorded type per
/// param), return type, and doc string. Enough to emit one Luau function
/// type.
struct Entry {
    name: String,
    params: Vec<(String, String)>,
    ret: String,
    doc: String,
}

/// What the declarations say of one function, as `neoc_macros::lua_fn` reads it from the Rust function that
/// implements it (`decode::lua()`): hand it to the module with the function, through [`Module::declared`] or one
/// of the `*_of` registrations, so the name, the parameters and the doc are written once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FnDecl {
    /// The name a script calls the function by.
    pub name: String,
    /// `(name, recorded type)` of each parameter.
    pub params: Vec<(String, String)>,
    /// The recorded return type.
    pub ret: String,
    pub doc: String,
}

impl FnDecl {
    /// The parameters as the registrations take them.
    pub(crate) fn borrowed(&self) -> Vec<(&str, &str)> {
        self.params
            .iter()
            .map(|(name, ty)| (name.as_str(), ty.as_str()))
            .collect()
    }
}

/// The recorded name of a trailing variadic parameter. A function that takes
/// any number of further arguments ends its `params` with
/// `("...", "LuaValue")`, which the declarations write as `...any`
/// (`(a: string, ...any) -> ()`).
const VARIADIC: &str = "...";

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
    functions: Vec<Entry>,
    fields: Vec<FieldEntry>,
}

/// One recorded value that is not a function: a module-level constant (for
/// example `std:thread`'s dynamically-injected `parent` / `shared`), a userdata
/// type's read-only field (`date.year`) or a constant on a type's sub-table
/// (`Duration.ZERO`). It is emitted as a property of a table type.
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
/// emit the matching Luau declarations with [`Module::luau_decls`].
pub struct Module<'l> {
    lua: &'l Lua,
    table: Table,
    name: String,
    entries: Vec<Entry>,
    userdata: Vec<UserDataEntry>,
    types: Vec<TypeEntry>,
    consts: Vec<FieldEntry>,
    /// Constants set on a type's own table before the type is declared: each waits for its type.
    type_consts: Vec<(String, FieldEntry)>,
}

impl<'l> Module<'l> {
    /// Creates a new module builder with an empty backing table.
    ///
    /// The `name` is the registry key the module is installed under
    /// (for example, `"lib:base64"`) and names the module's section of the
    /// declarations and the prefix of its type names (`lib_base64`).
    pub fn new(lua: &'l Lua, name: impl Into<String>) -> mlua::Result<Self> {
        Ok(Self {
            lua,
            table: lua.create_table()?,
            name: name.into(),
            entries: Vec::new(),
            userdata: Vec::new(),
            types: Vec::new(),
            consts: Vec::new(),
            type_consts: Vec::new(),
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

    /// [`Module::func`], with the name, the parameters and the doc read from `f` itself (`#[lua_fn]`).
    ///
    /// # Errors
    /// As [`Module::func`].
    pub fn func_of<A, R>(
        &mut self,
        decl: &FnDecl,
        f: impl Fn(&Lua, A) -> mlua::Result<R> + Send + 'static,
    ) -> mlua::Result<&mut Self>
    where
        A: FromLuaMulti,
        R: DeclType + IntoLua,
    {
        self.func(&decl.name, &decl.borrowed(), &decl.doc, f)
    }

    /// Sets `value` (a function made elsewhere, with its own span and its own way of failing) under the name
    /// `decl` has and records `decl`: [`Module::set`] and [`Module::record_fn`] in one, for a function whose
    /// declaration is read from its implementation (`#[lua_fn]`).
    ///
    /// # Errors
    /// As [`Module::set`].
    pub fn declared(&mut self, decl: FnDecl, value: impl IntoLua) -> mlua::Result<&mut Self> {
        self.table.set(decl.name.as_str(), value)?;
        Ok(self.record(decl))
    }

    /// Records `decl` for a function that is set on the table elsewhere: [`Module::record_fn`] with what
    /// `#[lua_fn]` read of the function in place of a second writing of it.
    pub fn record(&mut self, decl: FnDecl) -> &mut Self {
        self.entries.push(Entry {
            name: decl.name,
            params: decl.params,
            ret: decl.ret,
            doc: decl.doc,
        });
        self
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
        self.try_func_raising(name, params, doc, move |lua, a| Ok(f(lua, a)))
    }

    /// Registers a fallible function of any arity that may also raise.
    ///
    /// [`Module::try_func`] for a function that refuses some arguments outright
    /// (a programming error, such as a value of a type it cannot take) besides
    /// the failures it reports: `f` returns `mlua::Result<Result<T, E>>`. An
    /// outer `Err` raises, unchanged, as it would from [`Module::func`]; an
    /// inner `Err(e)` becomes `(nil, "<stem>.<name>: <e>")` and an `Ok(v)`
    /// `(v, nil)`, exactly as [`Module::try_func`] marshals them. The span and
    /// the recorded signature are `try_func`'s.
    pub fn try_func_raising<A, T, E>(
        &mut self,
        name: &str,
        params: &[(&str, &str)],
        doc: &str,
        f: impl Fn(&Lua, A) -> mlua::Result<Result<T, E>> + Send + 'static,
    ) -> mlua::Result<&mut Self>
    where
        A: FromLuaMulti,
        T: DeclType + IntoLua,
        E: std::fmt::Display,
    {
        let prefix = format!("{}.{}", module_stem(&self.name), name);
        let span = dyn_span(&self.target(), &prefix);
        let func = self.lua.create_function(move |lua, a: A| {
            span.in_scope(|| match f(lua, a)? {
                Ok(v) => ok(v),
                Err(e) => from_err(&prefix, e),
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
        T: DeclType + IntoLua + Send + 'static,
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
                    Ok(v) => ok(v),
                    Err(e) => from_err(&prefix, e),
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
    /// [`UserDataSpec::static_const`]. A module passes what `neoc_macros` wrote
    /// (`Type::lua`, chained with `static_of`) and calls none of these itself. This only records the surface — the
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
        // the constants said where they were set come first, in the order they were set
        let (mine, others): (Vec<_>, Vec<_>) = std::mem::take(&mut self.type_consts)
            .into_iter()
            .partition(|(of, _)| of == type_name);
        self.type_consts = others;
        spec.entry
            .static_consts
            .splice(0..0, mine.into_iter().map(|(_, entry)| entry));
        self.userdata.push(spec.entry);
        self
    }

    /// Records a constant set on the sub-table of the userdata type `type_name` (`Duration.ZERO`), where it
    /// is set: `neoc_macros::lua_module` writes this for `#[lua("Duration", constant, on(m, "Duration"))]`.
    /// The type is declared after it with [`Module::userdata`]; the declarations refuse a constant whose type
    /// never is.
    pub fn type_const(&mut self, type_name: &str, name: &str, ty: &str, doc: &str) -> &mut Self {
        self.type_consts.push((
            type_name.to_string(),
            FieldEntry {
                name: name.to_string(),
                ty: ty.to_string(),
                doc: doc.to_string(),
            },
        ));
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
    /// would. The recorded declarations are a `<Type>` alias of methods and a
    /// `<Type>Statics` alias of constructors.
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
            functions: Vec::new(),
            fields: Vec::new(),
        };
        build(&mut spec)?;
        let entry = TypeEntry {
            type_name: spec.type_name,
            constructors: spec.constructors,
            methods: spec.methods,
            functions: spec.functions,
            fields: spec.fields,
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
    /// only records the surface (a property of the module's table type); the
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

    /// What is recorded for each resource type of the module:
    /// `(type, methods, fields)`, so a test can hold the records to a live
    /// instance (a recorded method the type does not have would be a call
    /// the editor accepts and the VM refuses).
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
                    "{}: {}.{key} is on the module table but not recorded (hand its `#[lua_fn]` to `static_of`, or mark where a constant is set with `#[lua(\"T\", constant, on(m, \"Type\"))]`)",
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

    /// Records one function on the type's sub-table of the module table from what `#[lua_fn]` read of the
    /// function that implements it: [`UserDataSpec::static_fn`] without the second writing.
    pub fn static_of(&mut self, decl: FnDecl) -> &mut Self {
        self.entry.statics.push(Entry {
            name: decl.name,
            params: decl.params,
            ret: decl.ret,
            doc: decl.doc,
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

/// What records the members of a type: a userdata type's recorder ([`UserDataSpec`]) and a constructor-bearing
/// type's ([`TypeSpec`]). `Type::lua(spec)`, which `neoc_macros::lua_userdata` writes from the type's
/// `impl UserData`, records through it, so one type is declared the same way under either.
pub trait Members {
    /// Records one method: as [`UserDataSpec::method`].
    fn method(&mut self, name: &str, params: &[(&str, &str)], ret: &str, doc: &str);
    /// Records one function with no receiver: as [`UserDataSpec::function`].
    fn function(&mut self, name: &str, params: &[(&str, &str)], ret: &str, doc: &str);
    /// Records one read-only field: as [`UserDataSpec::field`].
    fn field(&mut self, name: &str, ty: &str, doc: &str);
}

impl Members for UserDataSpec {
    fn method(&mut self, name: &str, params: &[(&str, &str)], ret: &str, doc: &str) {
        UserDataSpec::method(self, name, params, ret, doc);
    }

    fn function(&mut self, name: &str, params: &[(&str, &str)], ret: &str, doc: &str) {
        UserDataSpec::function(self, name, params, ret, doc);
    }

    fn field(&mut self, name: &str, ty: &str, doc: &str) {
        UserDataSpec::field(self, name, ty, doc);
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
    functions: Vec<Entry>,
    fields: Vec<FieldEntry>,
}

impl Members for TypeSpec<'_, '_> {
    fn method(&mut self, name: &str, params: &[(&str, &str)], ret: &str, doc: &str) {
        TypeSpec::method(self, name, params, ret, doc);
    }

    fn function(&mut self, name: &str, params: &[(&str, &str)], ret: &str, doc: &str) {
        self.functions.push(Entry {
            name: name.to_string(),
            params: own_params(params),
            ret: ret.to_string(),
            doc: doc.to_string(),
        });
    }

    fn field(&mut self, name: &str, ty: &str, doc: &str) {
        self.fields.push(FieldEntry {
            name: name.to_string(),
            ty: ty.to_string(),
            doc: doc.to_string(),
        });
    }
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
                    Ok(v) => ok(v),
                    Err(e) => from_err(&prefix, e),
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

    use mlua::Lua;
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
}
