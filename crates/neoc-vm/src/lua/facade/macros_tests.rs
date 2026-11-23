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
#[neoc_macros::lua_fn(takes("text: string", "...: LuaValue"))]
fn parse(_: &Lua, text: String) -> mlua::Result<f64> {
    text.parse().map_err(mlua::Error::external)
}

/// Twice the number.
#[neoc_macros::lua_fn]
fn twice(_: &Lua, n: f64) -> Result<f64, String> {
    Ok(n * 2.0)
}

struct Counter(f64);

#[neoc_macros::lua_userdata("vnd:demo", "Counter")]
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
        Counter::lua(t);
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
fn a_type_takes_the_members_of_an_impl_too() {
    let decls = declarations();
    let maker = decls
        .split("export type vnd_demo_Maker = {")
        .nth(1)
        .unwrap_or_default();
    says(maker, "\tvalue: number,\n");
    says(
        maker,
        "\tadd: (self: vnd_demo_Maker, by: number) -> number,\n",
    );
    says(maker, "\tzero: () -> ");
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

// ---- the forms for what a signature does not show ----------------------------------------------------

const OPTIONS: &str = "{ deep: boolean? }";

fn waits(doc: &str) -> String {
    format!("{doc} It waits.")
}

/// A gauge at zero.
#[neoc_macros::lua_fn(name = "new")]
fn gauge_new(_: &Lua, (): ()) -> mlua::Result<Gauge> {
    Ok(Gauge(0.0))
}

/// A gauge at `from`.
#[neoc_macros::lua_fn(registered)]
fn gauge_from(_: &Lua, from: f64) -> mlua::Result<Gauge> {
    Ok(Gauge(from))
}

struct Gauge(f64);

fn add_reads<M: UserDataMethods<Gauge>>(methods: &mut M) {
    methods.add_method("read", |_, this, ()| Ok(this.0));
    methods.add_method("read_twice", |_, this, ()| Ok(this.0 * 2.0));
}

#[neoc_macros::lua_userdata("vnd:more", "Gauge")]
impl UserData for Gauge {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        /// The reading.
        #[lua("number", name = "read", takes())]
        /// Twice the reading.
        #[lua("number", name = "read_twice", takes(opts = format!("{OPTIONS}?")))]
        add_reads(methods);
        /// How far this is from `other`.
        #[lua("number", method, takes("other: Gauge"))]
        methods.add_function(
            "gap",
            |_, (a, b): (mlua::AnyUserData, mlua::AnyUserData)| {
                Ok(a.borrow::<Gauge>()?.0 - b.borrow::<Gauge>()?.0)
            },
        );
    }
}

/// A table of checks on `value`.
#[neoc_macros::lua_members]
fn checks(lua: &Lua, value: f64) -> mlua::Result<mlua::Table> {
    let table = lua.create_table()?;
    /// Whether the value is above `floor`.
    #[lua("bool")]
    check_function(lua, &table, "above", value, |value: f64, floor: f64| {
        value > floor
    })?;
    /// Whether the value is zero.
    #[lua("bool", name = "zero", function, takes())]
    table.set("zero", value == 0.0)?;
    Ok(table)
}

fn check_function(
    lua: &Lua,
    table: &mlua::Table,
    name: &str,
    value: f64,
    check: impl Fn(f64, f64) -> bool + Send + 'static,
) -> mlua::Result<()> {
    table.set(
        name,
        lua.create_function(move |_, floor: f64| Ok(check(value, floor)))?,
    )
}

#[neoc_macros::lua_module]
fn declare_more(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "vnd:more")?;
    /// The largest reading.
    #[lua("number", constant)]
    m.set("LIMIT", 100)?;
    for (name, doc) in [("slow", "Read slowly."), ("slower", "Read more slowly.")] {
        #[lua(returns = "number", types(_opts = format!("{OPTIONS}?")), doc = doc, doc_with = waits)]
        m.set(
            name,
            lua.create_function(|_, _opts: Option<mlua::Table>| Ok(1))?,
        )?;
    }
    let sub = lua.create_table()?;
    sub.set("new", lua.create_function(gauge_new)?)?;
    m.set("Gauge", sub)?;
    m.userdata("Gauge", |g| {
        Gauge::lua(g);
        g.static_of(gauge_new::lua());
    });
    m.type_("Dial", |t| {
        #[lua]
        t.constructor("from", gauge_from)?;
        Ok(())
    })?;
    m.userdata("Checks", checks_members);
    Ok(m)
}

fn more() -> String {
    let lua = Lua::new();
    declare_more(&lua)
        .expect("declares")
        .luau_decls()
        .expect("declarations")
}

#[test]
fn a_constant_is_declared_where_it_is_set() {
    says(&more(), "\t--- The largest reading.\n\tLIMIT: number,\n");
}

#[test]
fn a_name_a_type_and_a_doc_can_be_put_together() {
    let decls = more();
    says(
        &decls,
        "\t--- Read slowly. It waits.\n\tslow: (opts: { deep: boolean? }?) -> number,\n",
    );
    says(
        &decls,
        "\t--- Read more slowly. It waits.\n\tslower: (opts: { deep: boolean? }?) -> number,\n",
    );
}

#[test]
fn members_added_out_of_sight_are_said_above_the_statement() {
    let decls = more();
    says(
        &decls,
        "\t--- The reading.\n\tread: (self: vnd_more_Gauge) -> number,\n",
    );
    says(
        &decls,
        "\t--- Twice the reading.\n\tread_twice: (self: vnd_more_Gauge, opts: { deep: boolean? }?) -> number,\n",
    );
    says(
        &decls,
        "\t--- How far this is from `other`.\n\tgap: (self: vnd_more_Gauge, other: vnd_more_Gauge) -> number,\n",
    );
}

#[test]
fn a_static_and_a_constructor_are_declared_from_their_functions() {
    let decls = more();
    says(
        &decls,
        "\t--- A gauge at zero.\n\tnew: () -> vnd_more_Gauge,\n",
    );
    says(
        &decls,
        "\t--- A gauge at `from`.\n\tfrom: (from: number) -> ",
    );
}

#[test]
fn a_table_a_function_builds_declares_its_members() {
    let decls = more();
    says(
        &decls,
        "\t--- Whether the value is above `floor`.\n\tabove: (floor: number) -> boolean,\n",
    );
    says(
        &decls,
        "\t--- Whether the value is zero.\n\tzero: () -> boolean,\n",
    );
    let lua = Lua::new();
    assert!(checks(&lua, 0.0).is_ok());
}

/// A module whose type carries a constant on its own table; `declared` says whether the type is.
#[neoc_macros::lua_module]
fn declare_origin(lua: &Lua, declared: bool) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "vnd:origin")?;
    let counter = lua.create_table()?;
    /// The counter nothing was added to.
    #[lua("Counter", constant, on(m, "Counter"))]
    counter.set("ORIGIN", Counter(0.0))?;
    m.set("Counter", counter)?;
    if declared {
        m.userdata("Counter", Counter::lua);
    }
    Ok(m)
}

#[test]
fn a_constant_of_a_type_is_declared_on_the_type_where_it_is_set() {
    let lua = Lua::new();
    let decls = declare_origin(&lua, true)
        .expect("declares")
        .luau_decls()
        .expect("declarations");
    says(
        &decls,
        "\t--- The counter nothing was added to.\n\tORIGIN: vnd_origin_Counter,\n",
    );
}

#[test]
fn a_constant_of_a_type_the_module_never_declares_is_refused() {
    let lua = Lua::new();
    let error = declare_origin(&lua, false)
        .expect("declares")
        .luau_decls()
        .expect_err("the type is never declared")
        .to_string();
    assert!(
        error.contains(
            "the constant `ORIGIN` is of the type `Counter`, which the module never declares"
        ),
        "{error}"
    );
}

/// A type whose members say nothing of what they return or take: the compiler does.
struct Probe(f64);

// the test registers with mlua directly: what is under test is the reading of the types, not the scheduling
#[allow(clippy::disallowed_methods)]
#[neoc_macros::lua_userdata("vnd:typed", "Probe")]
impl UserData for Probe {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        /// The reading.
        fields.add_field_method_get("reading", |_, this| Ok(this.0));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        /// The reading as text, when there is one.
        methods.add_method("text", |_, this, ()| {
            Ok((this.0 != 0.0).then(|| this.0.to_string()))
        });
        /// A probe moved by `by`.
        methods.add_method("moved", |_, this, by: f64| Ok(Probe(this.0 + by)));
        /// The reading over `by`, or why not.
        methods.add_method("over", |_, this, by: f64| {
            if by == 0.0 {
                return crate::lua::sandbox::err("probe.over: by zero");
            }
            crate::lua::sandbox::ok(this.0 / by)
        });
        /// How far this is from `other`.
        methods.add_method("gap", |_, this, other: super::Handle<Probe>| {
            Ok(this.0 - other.borrow::<Probe>()?.0)
        });
        /// Every reading so far.
        methods.add_method("history", |_, this, ()| Ok(super::List(vec![this.0])));
        /// The reading, later.
        methods.add_async_method("later", |_, this, ()| async move { Ok(this.0) });
        // a metamethod is no member a script names
        methods.add_meta_method(mlua::MetaMethod::ToString, |_, this, ()| {
            Ok(this.0.to_string())
        });
    }
}

#[allow(clippy::disallowed_methods)]
#[neoc_macros::lua_module]
fn declare_typed(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "vnd:typed")?;
    /// A probe at `at`.
    #[lua]
    m.set("probe", lua.create_function(|_, at: f64| Ok(Probe(at)))?)?;
    /// A probe at zero, later.
    #[lua]
    m.set(
        "probe_later",
        lua.create_async_function(|_, ()| async move { Ok(Probe(0.0)) })?,
    )?;
    /// The most a probe reads.
    #[lua(constant)]
    m.set("LIMIT", 100.0)?;
    m.userdata("Probe", Probe::lua);
    Ok(m)
}

#[test]
fn what_a_closure_returns_and_takes_is_read_from_its_types() {
    let lua = Lua::new();
    let decls = declare_typed(&lua)
        .expect("declares")
        .luau_decls()
        .expect("declarations");
    for line in [
        "\t--- The reading.\n\treading: number,\n",
        "\t--- The reading as text, when there is one.\n\ttext: (self: vnd_typed_Probe) -> string?,\n",
        "\t--- A probe moved by `by`.\n\tmoved: (self: vnd_typed_Probe, by: number) -> vnd_typed_Probe,\n",
        "\t--- The reading over `by`, or why not.\n\tover: (self: vnd_typed_Probe, by: number) -> (number?, string?),\n",
        "\t--- How far this is from `other`.\n\tgap: (self: vnd_typed_Probe, other: vnd_typed_Probe) -> number,\n",
        "\t--- Every reading so far.\n\thistory: (self: vnd_typed_Probe) -> { number },\n",
        "\t--- The reading, later.\n\tlater: (self: vnd_typed_Probe) -> number,\n",
        "\t--- A probe at `at`.\n\tprobe: (at: number) -> vnd_typed_Probe,\n",
        "\t--- A probe at zero, later.\n\tprobe_later: () -> vnd_typed_Probe,\n",
        "\t--- The most a probe reads.\n\tLIMIT: number,\n",
    ] {
        says(&decls, line);
    }
    assert!(!decls.contains("__tostring"), "{decls}");
}

#[test]
fn a_type_is_registered_as_it_was_before_its_members_were_noted() {
    // noting is for the declarations alone: with mlua, the members are the ones the block adds
    let lua = Lua::new();
    lua.globals().set("probe", Probe(6.0)).expect("set");
    let read: (f64, String, f64, mlua::Value, mlua::Value) = lua
        .load("local n, e = probe:over(0) return probe.reading, probe:text(), probe:moved(1).reading, n, e")
        .eval()
        .expect("runs");
    assert_eq!((read.0, read.1.as_str(), read.2), (6.0, "6", 7.0));
    assert!(read.3.is_nil());
    assert_eq!(read.4.to_string().expect("text"), "probe.over: by zero");
}
