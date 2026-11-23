//! What `neoc-macros` reads from an implementation: each test declares a small module the way a real one
//! does and checks the declarations it comes to.

use mlua::{Lua, UserData, UserDataFields, UserDataMethods, Variadic};

use super::Module;

/// Join two strings.
/// The second line of the doc.
#[neoc_macros::lua_fn]
fn join(_: &Lua, (a, b): (String, String)) -> mlua::Result<String> {
    Ok(format!("{a}{b}"))
}

// a plain comment is not the script's doc
/// Count what is passed.
#[neoc_macros::lua_fn(name = "count", names(_first = "first"), types(rest = "LuaValue"))]
fn count_all(_first: Option<f64>, rest: Variadic<mlua::Value>) -> mlua::Result<usize> {
    Ok(rest.len())
}

/// Parse a number.
#[neoc_macros::lua_fn(
    returns = "Result<number, string>",
    takes("text: string", "...: LuaValue")
)]
fn parse(_: &Lua, text: String) -> mlua::Result<f64> {
    text.parse().map_err(mlua::Error::external)
}

/// Twice the number.
#[neoc_macros::lua_fn]
fn twice(_: &Lua, n: f64) -> Result<f64, String> {
    Ok(n * 2.0)
}

struct Counter(f64);

#[neoc_macros::lua_userdata]
impl UserData for Counter {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        /// The count so far.
        #[lua("number")]
        fields.add_field_method_get("value", |_, this| Ok(this.0));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        /// Add `by` and return the new count.
        #[lua("number")]
        methods.add_method_mut("add", |_, this, by: f64| {
            this.0 += by;
            Ok(this.0)
        });
        #[lua("()", names(_why = "why"), types(_why = "string?"))]
        methods.add_method_mut("reset", |_, this, _why: Option<String>| {
            this.0 = 0.0;
            Ok(())
        });
        /// A counter from nothing.
        #[lua("Counter")]
        methods.add_function("zero", |_, ()| Ok(Counter(0.0)));
        #[lua(skip)]
        methods.add_method("hidden", |_, this, ()| Ok(this.0));
        methods.add_meta_method(mlua::MetaMethod::ToString, |_, this, ()| {
            Ok(this.0.to_string())
        });
    }
}

#[neoc_macros::lua_module]
fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "vnd:demo")?;
    m.declared(join::lua(), lua.create_function(join)?)?;
    m.set(
        "count",
        lua.create_function(|_, (first, rest)| count_all(first, rest))?,
    )?;
    m.record(count_all::lua());
    m.func_of(&parse::lua(), parse)?;
    #[lua]
    m.try_func("twice", twice)?;
    /// Upper case.
    #[lua]
    m.func("upper", |_, text: String| Ok(text.to_uppercase()))?;
    /// The length, or why not.
    #[lua(names(_strict = "strict"))]
    m.try_func("len", |_, (text, _strict): (String, Option<bool>)| {
        Ok::<_, String>(text.len())
    })?;
    /// Always one.
    #[lua("number", takes("...: LuaValue"))]
    m.set("one", lua.create_function(|_, _: mlua::MultiValue| Ok(1))?)?;
    /// The number given, back.
    #[lua("number")]
    m.set("same", lua.create_function(|_, n: f64| Ok(n))?)?;
    m.userdata("Counter", Counter::lua);
    m.type_("Maker", |t| {
        /// A counter that starts at `from`.
        #[lua]
        t.constructor("starting", |_, from: f64| Ok(from))?;
        Ok(())
    })?;
    Ok(m)
}

fn declarations() -> String {
    let lua = Lua::new();
    declare(&lua)
        .expect("declares")
        .luau_decls()
        .expect("declarations")
}

#[track_caller]
fn says(decls: &str, line: &str) {
    assert!(decls.contains(line), "`{line}` is not in:\n{decls}");
}

#[test]
fn a_function_is_declared_from_its_signature_and_its_doc() {
    let decls = declarations();
    says(
        &decls,
        "\t--- Join two strings.\n\t--- The second line of the doc.\n\tjoin: (a: string, b: string) -> string,\n",
    );
    assert!(!decls.contains("plain comment"), "{decls}");
}

#[test]
fn a_name_a_type_and_a_parameter_name_can_be_said() {
    says(
        &declarations(),
        "\t--- Count what is passed.\n\tcount: (first: number?, ...any) -> number,\n",
    );
}

#[test]
fn a_parameter_list_and_a_return_can_be_said_whole() {
    says(&declarations(), "\tparse: (text: string, ...any) -> ");
}

#[test]
fn a_registration_given_a_function_takes_its_parameters_and_doc_from_it() {
    says(
        &declarations(),
        "\t--- Twice the number.\n\ttwice: (n: number) -> (number?, string?),\n",
    );
}

#[test]
fn a_registration_given_a_closure_takes_its_parameters_from_it() {
    let decls = declarations();
    says(
        &decls,
        "\t--- Upper case.\n\tupper: (text: string) -> string,\n",
    );
    says(
        &decls,
        "\t--- The length, or why not.\n\tlen: (text: string, strict: boolean?) -> (number?, string?),\n",
    );
}

#[test]
fn a_set_is_recorded_with_the_return_it_says() {
    let decls = declarations();
    says(&decls, "\t--- Always one.\n\tone: (...any) -> number,\n");
    says(
        &decls,
        "\t--- The number given, back.\n\tsame: (n: number) -> number,\n",
    );
}

#[test]
fn the_members_of_a_userdata_type_are_declared_from_its_impl() {
    let decls = declarations();
    says(&decls, "\t--- The count so far.\n\tvalue: number,\n");
    says(
        &decls,
        "\t--- Add `by` and return the new count.\n\tadd: (self: vnd_demo_Counter, by: number) -> number,\n",
    );
    says(
        &decls,
        "\treset: (self: vnd_demo_Counter, why: string?) -> (),\n",
    );
    says(
        &decls,
        "\t--- A counter from nothing.\n\tzero: () -> vnd_demo_Counter,\n",
    );
    assert!(!decls.contains("hidden"), "{decls}");
    assert!(!decls.contains("tostring"), "{decls}");
}

#[test]
fn a_constructor_in_a_type_is_declared_from_its_closure() {
    says(
        &declarations(),
        "\t--- A counter that starts at `from`.\n\tstarting: (from: number) -> ",
    );
}

#[test]
fn what_is_declared_is_what_runs() {
    let lua = Lua::new();
    let table = declare(&lua).expect("declares").into_table();
    lua.globals().set("demo", table).expect("set");
    let out: (String, usize, f64, String, f64) = lua
        .load(r#"return demo.join("a", "b"), demo.count(1, 2, 3), demo.twice(4), demo.upper("x"), demo.same(7)"#)
        .eval()
        .expect("runs");
    assert_eq!(out, ("ab".to_string(), 2, 8.0, "X".to_string(), 7.0));
}
