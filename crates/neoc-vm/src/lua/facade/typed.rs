//! Members whose types the compiler works out.
//!
//! A closure given to `add_method` says its parameters in its own text, but what it returns is known only to the
//! compiler. `neoc_macros::lua_userdata` therefore passes each closure through one of the functions here on its
//! way to mlua (`methods.add_method("len", typed::add_method(|_, this, ()| Ok(this.len()), ...))`). The function
//! hands the closure back untouched; its bounds make the compiler name the return type, and that type's
//! [`DeclType`] is the declaration. Nothing about the return is written beside the closure.
//!
//! The note is taken only while a type's members are being recorded ([`record`]), which runs the type's own
//! `add_fields` and `add_methods` against a registry that registers nothing. A real registration with mlua
//! passes through the same functions and notes nothing.

use std::cell::RefCell;
use std::future::Future;
use std::marker::PhantomData;

use mlua::{
    AnyUserData, FromLua, FromLuaMulti, IntoLua, IntoLuaMulti, Lua, UserData, UserDataFields,
    UserDataMethods, UserDataRef, UserDataRefMut,
};

use super::{DeclType, Members};

/// One member as it was noted: what [`Members`] is told of it.
enum Noted {
    Method(Signature),
    Function(Signature),
    Field {
        name: String,
        ty: String,
        doc: String,
    },
}

struct Signature {
    name: String,
    params: Vec<(String, String)>,
    ret: String,
    doc: String,
}

thread_local! {
    /// The members noted so far, while a type is being recorded; `None` at every other time.
    static NOTES: RefCell<Option<Vec<Noted>>> = const { RefCell::new(None) };
}

fn note(member: impl FnOnce() -> Noted) {
    NOTES.with_borrow_mut(|notes| {
        if let Some(notes) = notes {
            notes.push(member());
        }
    });
}

/// What a member that takes arguments is noted with: its name, its parameters and its doc, as the macro read
/// them from the statement. The parameters are built only when a note is taken.
pub struct Of<'a, P: FnOnce() -> Vec<(String, String)>> {
    pub name: &'a str,
    pub params: P,
    pub doc: &'a str,
}

impl<P: FnOnce() -> Vec<(String, String)>> Of<'_, P> {
    fn signature<R: DeclType>(self) -> Signature {
        Signature {
            name: self.name.to_owned(),
            params: (self.params)(),
            ret: R::decl_type(),
            doc: self.doc.to_owned(),
        }
    }
}

/// Notes a member whose return is said by hand, or that is added where the macro cannot pass a closure through
/// (a helper, a loop): `kind` is `"method"`, `"function"` or `"field"`.
pub fn said(kind: &str, name: &str, params: Vec<(String, String)>, ret: &str, doc: &str) {
    note(|| {
        let signature = || Signature {
            name: name.to_owned(),
            params,
            ret: ret.to_owned(),
            doc: doc.to_owned(),
        };
        match kind {
            "field" => Noted::Field {
                name: name.to_owned(),
                ty: ret.to_owned(),
                doc: doc.to_owned(),
            },
            "function" => Noted::Function(signature()),
            _ => Noted::Method(signature()),
        }
    });
}

/// The closure of `add_method`.
pub fn add_method<T, A, R, M, P>(method: M, of: Of<'_, P>) -> M
where
    M: Fn(&Lua, &T, A) -> mlua::Result<R>,
    R: DeclType,
    P: FnOnce() -> Vec<(String, String)>,
{
    note(|| Noted::Method(of.signature::<R>()));
    method
}

/// The closure of `add_method_mut`.
pub fn add_method_mut<T, A, R, M, P>(method: M, of: Of<'_, P>) -> M
where
    M: FnMut(&Lua, &mut T, A) -> mlua::Result<R>,
    R: DeclType,
    P: FnOnce() -> Vec<(String, String)>,
{
    note(|| Noted::Method(of.signature::<R>()));
    method
}

/// The closure of `add_async_method`.
pub fn add_async_method<T: 'static, A, R, MR, M, P>(method: M, of: Of<'_, P>) -> M
where
    M: Fn(Lua, UserDataRef<T>, A) -> MR,
    MR: Future<Output = mlua::Result<R>>,
    R: DeclType,
    P: FnOnce() -> Vec<(String, String)>,
{
    note(|| Noted::Method(of.signature::<R>()));
    method
}

/// The closure of `add_async_method_mut`.
pub fn add_async_method_mut<T: 'static, A, R, MR, M, P>(method: M, of: Of<'_, P>) -> M
where
    M: Fn(Lua, UserDataRefMut<T>, A) -> MR,
    MR: Future<Output = mlua::Result<R>>,
    R: DeclType,
    P: FnOnce() -> Vec<(String, String)>,
{
    note(|| Noted::Method(of.signature::<R>()));
    method
}

/// The closure of `add_function`.
pub fn add_function<A, R, F, P>(function: F, of: Of<'_, P>) -> F
where
    F: Fn(&Lua, A) -> mlua::Result<R>,
    R: DeclType,
    P: FnOnce() -> Vec<(String, String)>,
{
    note(|| Noted::Function(of.signature::<R>()));
    function
}

/// The closure of `add_function_mut`.
pub fn add_function_mut<A, R, F, P>(function: F, of: Of<'_, P>) -> F
where
    F: FnMut(&Lua, A) -> mlua::Result<R>,
    R: DeclType,
    P: FnOnce() -> Vec<(String, String)>,
{
    note(|| Noted::Function(of.signature::<R>()));
    function
}

/// The closure of `add_async_function`.
pub fn add_async_function<A, R, FR, F, P>(function: F, of: Of<'_, P>) -> F
where
    F: Fn(Lua, A) -> FR,
    FR: Future<Output = mlua::Result<R>>,
    R: DeclType,
    P: FnOnce() -> Vec<(String, String)>,
{
    note(|| Noted::Function(of.signature::<R>()));
    function
}

/// The closure of `add_field_method_get`.
pub fn add_field_method_get<T, R, M>(method: M, name: &str, doc: &str) -> M
where
    M: Fn(&Lua, &T) -> mlua::Result<R>,
    R: DeclType,
{
    note(|| Noted::Field {
        name: name.to_owned(),
        ty: R::decl_type(),
        doc: doc.to_owned(),
    });
    method
}

/// The closure of `add_field_function_get`.
pub fn add_field_function_get<R, F>(function: F, name: &str, doc: &str) -> F
where
    F: Fn(&Lua, AnyUserData) -> mlua::Result<R>,
    R: DeclType,
{
    note(|| Noted::Field {
        name: name.to_owned(),
        ty: R::decl_type(),
        doc: doc.to_owned(),
    });
    function
}

/// Records the members of `T` on `spec`, in the order its `add_fields` and then its `add_methods` add them.
///
/// # Panics
///
/// When called while another type is being recorded: a type's `add_methods` does not declare a second type.
pub fn record<T: UserData>(spec: &mut impl Members) {
    let before = NOTES.replace(Some(Vec::new()));
    assert!(
        before.is_none(),
        "a type's members are recorded one type at a time"
    );
    let mut nothing = Unregistered::<T>(PhantomData);
    T::add_fields(&mut nothing);
    T::add_methods(&mut nothing);
    for member in NOTES.take().unwrap_or_default() {
        match member {
            Noted::Method(s) => spec.method(&s.name, &borrowed(&s.params), &s.ret, &s.doc),
            Noted::Function(s) => spec.function(&s.name, &borrowed(&s.params), &s.ret, &s.doc),
            Noted::Field { name, ty, doc } => spec.field(&name, &ty, &doc),
        }
    }
}

fn borrowed(params: &[(String, String)]) -> Vec<(&str, &str)> {
    params
        .iter()
        .map(|(name, ty)| (name.as_str(), ty.as_str()))
        .collect()
}

/// A registry that registers nothing: what a type's `add_fields` and `add_methods` are run against to note
/// their members.
struct Unregistered<T>(PhantomData<T>);

impl<T> UserDataFields<T> for Unregistered<T> {
    fn add_field<V>(&mut self, _: impl Into<String>, _: V)
    where
        V: IntoLua + 'static,
    {
    }

    fn add_field_method_get<M, R>(&mut self, _: impl Into<String>, _: M)
    where
        M: Fn(&Lua, &T) -> mlua::Result<R> + mlua::MaybeSend + 'static,
        R: IntoLua,
    {
    }

    fn add_field_method_set<M, A>(&mut self, _: impl Into<String>, _: M)
    where
        M: FnMut(&Lua, &mut T, A) -> mlua::Result<()> + mlua::MaybeSend + 'static,
        A: FromLua,
    {
    }

    fn add_field_function_get<F, R>(&mut self, _: impl Into<String>, _: F)
    where
        F: Fn(&Lua, AnyUserData) -> mlua::Result<R> + mlua::MaybeSend + 'static,
        R: IntoLua,
    {
    }

    fn add_field_function_set<F, A>(&mut self, _: impl Into<String>, _: F)
    where
        F: FnMut(&Lua, AnyUserData, A) -> mlua::Result<()> + mlua::MaybeSend + 'static,
        A: FromLua,
    {
    }

    fn add_meta_field<V>(&mut self, _: impl Into<String>, _: V)
    where
        V: IntoLua + 'static,
    {
    }

    fn add_meta_field_with<F, R>(&mut self, _: impl Into<String>, _: F)
    where
        F: FnOnce(&Lua) -> mlua::Result<R> + 'static,
        R: IntoLua,
    {
    }
}

impl<T> UserDataMethods<T> for Unregistered<T> {
    fn add_method<M, A, R>(&mut self, _: impl Into<String>, _: M)
    where
        M: Fn(&Lua, &T, A) -> mlua::Result<R> + mlua::MaybeSend + 'static,
        A: FromLuaMulti,
        R: IntoLuaMulti,
    {
    }

    fn add_method_mut<M, A, R>(&mut self, _: impl Into<String>, _: M)
    where
        M: FnMut(&Lua, &mut T, A) -> mlua::Result<R> + mlua::MaybeSend + 'static,
        A: FromLuaMulti,
        R: IntoLuaMulti,
    {
    }

    fn add_async_method<M, A, MR, R>(&mut self, _: impl Into<String>, _: M)
    where
        T: 'static,
        M: Fn(Lua, UserDataRef<T>, A) -> MR + mlua::MaybeSend + 'static,
        A: FromLuaMulti,
        MR: Future<Output = mlua::Result<R>> + mlua::MaybeSend + 'static,
        R: IntoLuaMulti,
    {
    }

    fn add_async_method_mut<M, A, MR, R>(&mut self, _: impl Into<String>, _: M)
    where
        T: 'static,
        M: Fn(Lua, UserDataRefMut<T>, A) -> MR + mlua::MaybeSend + 'static,
        A: FromLuaMulti,
        MR: Future<Output = mlua::Result<R>> + mlua::MaybeSend + 'static,
        R: IntoLuaMulti,
    {
    }

    fn add_function<F, A, R>(&mut self, _: impl Into<String>, _: F)
    where
        F: Fn(&Lua, A) -> mlua::Result<R> + mlua::MaybeSend + 'static,
        A: FromLuaMulti,
        R: IntoLuaMulti,
    {
    }

    fn add_function_mut<F, A, R>(&mut self, _: impl Into<String>, _: F)
    where
        F: FnMut(&Lua, A) -> mlua::Result<R> + mlua::MaybeSend + 'static,
        A: FromLuaMulti,
        R: IntoLuaMulti,
    {
    }

    fn add_async_function<F, A, FR, R>(&mut self, _: impl Into<String>, _: F)
    where
        F: Fn(Lua, A) -> FR + mlua::MaybeSend + 'static,
        A: FromLuaMulti,
        FR: Future<Output = mlua::Result<R>> + mlua::MaybeSend + 'static,
        R: IntoLuaMulti,
    {
    }

    fn add_meta_method<M, A, R>(&mut self, _: impl Into<String>, _: M)
    where
        M: Fn(&Lua, &T, A) -> mlua::Result<R> + mlua::MaybeSend + 'static,
        A: FromLuaMulti,
        R: IntoLuaMulti,
    {
    }

    fn add_meta_method_mut<M, A, R>(&mut self, _: impl Into<String>, _: M)
    where
        M: FnMut(&Lua, &mut T, A) -> mlua::Result<R> + mlua::MaybeSend + 'static,
        A: FromLuaMulti,
        R: IntoLuaMulti,
    {
    }

    fn add_meta_function<F, A, R>(&mut self, _: impl Into<String>, _: F)
    where
        F: Fn(&Lua, A) -> mlua::Result<R> + mlua::MaybeSend + 'static,
        A: FromLuaMulti,
        R: IntoLuaMulti,
    {
    }

    fn add_meta_function_mut<F, A, R>(&mut self, _: impl Into<String>, _: F)
    where
        F: FnMut(&Lua, A) -> mlua::Result<R> + mlua::MaybeSend + 'static,
        A: FromLuaMulti,
        R: IntoLuaMulti,
    {
    }
}
