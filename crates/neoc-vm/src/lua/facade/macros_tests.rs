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

#[test]
fn a_function_that_can_fail_hands_back_two_values() {
    let lua = Lua::new();
    lua.globals().set("probe", Probe(6.0)).expect("set");
    let counts: (i64, i64) = lua
        .load("return select('#', probe:over(2)), select('#', probe:over(0))")
        .eval()
        .expect("runs");
    assert_eq!(counts, (2, 2));
}

#[test]
fn a_recording_that_panics_leaves_no_notes_behind() {
    struct Broken;
    impl UserData for Broken {
        fn add_methods<M: UserDataMethods<Self>>(_: &mut M) {
            panic!("broken on purpose");
        }
    }
    struct Spec(Vec<String>);
    impl super::Members for Spec {
        fn method(&mut self, name: &str, _: &[(&str, &str)], _: &str, _: &str) {
            self.0.push(name.to_owned());
        }
        fn function(&mut self, name: &str, _: &[(&str, &str)], _: &str, _: &str) {
            self.0.push(name.to_owned());
        }
        fn field(&mut self, name: &str, _: &str, _: &str) {
            self.0.push(name.to_owned());
        }
    }
    let mut spec = Spec(Vec::new());
    let broken = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        super::typed::record::<Broken>(&mut Spec(Vec::new()));
    }));
    assert!(broken.is_err());
    // the next type records as if nothing had happened
    super::typed::record::<Probe>(&mut spec);
    assert_eq!(spec.0.first().map(String::as_str), Some("reading"));
}

/// What a reading left: a table to a script, and the declaration is its fields.
#[neoc_macros::lua_record(into, from)]
struct Left {
    code: Option<i64>,
    r#type: String,
}

#[test]
fn a_record_is_a_table_both_ways_and_refuses_what_is_not_one() {
    use super::DeclType;
    assert_eq!(Left::decl_type(), "{ code: number?, type: string }");
    let lua = Lua::new();
    let left = Left {
        code: None,
        r#type: "done".to_owned(),
    };
    lua.globals().set("left", left).expect("set");
    let read = |code: &str| {
        let value: mlua::Value = lua.load(code).eval().expect("eval");
        <Left as super::args::Argument>::read(value, &lua).expect("nothing is raised")
    };
    let back =
        read("assert(left.code == nil and left.type == 'done'); return { code = 3, type = 'x' }")
            .expect("a table of the fields is the record");
    assert_eq!((back.code, back.r#type.as_str()), (Some(3), "x"));
    let refused = |code: &str| read(code).map_or_else(|refused| refused.0, |_| String::new());
    assert_eq!(
        refused("return { type = 'x', kind = 1 }"),
        "unknown option \"kind\" (this function takes: code, type)"
    );
    assert_eq!(refused("return 7"), "options must be a table, got integer");
    assert_eq!(
        refused("return { code = 3 }"),
        "option \"type\" must be a string"
    );
    assert_eq!(
        refused("return { code = 1.5, type = 'x' }"),
        "option \"code\" must be a whole number"
    );
    // a table is read raw: an `__index` is not asked
    assert_eq!(
        refused("return setmetatable({}, { __index = function() error('ran') end })"),
        "option \"type\" must be a string"
    );
}

/// A type whose member a helper adds: the statement says the name and the parameters, the helper the return.
struct Helped;

fn helped<M: UserDataMethods<Helped>, R>(methods: &mut M, name: &str, value: fn() -> R)
where
    R: mlua::IntoLua + super::DeclType + 'static,
{
    super::typed::answers::<R>();
    #[allow(clippy::disallowed_methods)]
    methods.add_method(name, move |_, _, ()| Ok(value()));
}

fn silent<M: UserDataMethods<Unanswered>>(_: &mut M) {}

#[neoc_macros::lua_userdata("vnd:typed", "Helped")]
impl UserData for Helped {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        /// How many.
        #[lua(name = "count", takes())]
        helped(methods, "count", || 3_i64);
        /// What was left.
        #[lua(name = "left", takes("after: number"))]
        helped(methods, "left", || None::<String>);
    }
}

struct Unanswered;

#[neoc_macros::lua_userdata]
impl UserData for Unanswered {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        #[lua(name = "lost", takes())]
        silent(methods);
    }
}

#[test]
fn a_helper_answers_for_the_member_it_adds_and_silence_is_refused() {
    struct Spec(Vec<String>);
    impl super::Members for Spec {
        fn method(&mut self, name: &str, params: &[(&str, &str)], ret: &str, _: &str) {
            self.0.push(format!("{name}({}) -> {ret}", params.len()));
        }
        fn function(&mut self, _: &str, _: &[(&str, &str)], _: &str, _: &str) {}
        fn field(&mut self, _: &str, _: &str, _: &str) {}
    }
    let mut spec = Spec(Vec::new());
    Helped::lua(&mut spec);
    // each helper call answers the first member still without an answer, so two members said above one
    // statement get their own types in order
    assert_eq!(spec.0, ["count(0) -> number", "left(1) -> string?"]);
    let lost = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Unanswered::lua(&mut Spec(Vec::new()));
    }));
    let message = lost
        .err()
        .and_then(|e| e.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert!(
        message.contains("`lost`: nothing says what it returns"),
        "{message}"
    );
}

#[test]
fn an_optional_union_is_bracketed_and_nothing_else_is() {
    use super::DeclType;
    struct Either;
    impl DeclType for Either {
        fn decl_type() -> String {
            r#""a | b" | number"#.to_owned()
        }
    }
    assert_eq!(Option::<Either>::decl_type(), r#"("a | b" | number)?"#);
    assert_eq!(
        Option::<Left>::decl_type(),
        "{ code: number?, type: string }?"
    );
    assert_eq!(
        Option::<super::Declared<super::Map<String, Either>>>::decl_type(),
        r#"{ [string]: "a | b" | number }?"#
    );
}

/// A type whose methods are real methods: everything the declarations say is their signatures.
#[derive(Clone)]
struct Valve(i64);

#[derive(Debug)]
struct ValveError(String);

impl std::fmt::Display for ValveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid_input: {}", self.0)
    }
}

impl From<super::args::Refused> for ValveError {
    fn from(refused: super::args::Refused) -> Self {
        Self(refused.0)
    }
}

#[neoc_macros::lua_record(from)]
struct Turn {
    by: Option<i64>,
    slowly: Option<bool>,
}

#[neoc_macros::lua_methods("vnd:typed", "Valve")]
impl Valve {
    /// How far it is open.
    fn opening(&self) -> i64 {
        self.0
    }

    /// Turn it, and say where it is now.
    fn turn(&self, name: String, opts: Option<Turn>) -> Result<i64, ValveError> {
        let Turn { by, slowly } = opts.unwrap_or(Turn {
            by: None,
            slowly: None,
        });
        if name.is_empty() {
            return Err(ValveError("no name".to_owned()));
        }
        Ok(self.0 + by.unwrap_or(1) * if slowly == Some(true) { 1 } else { 2 })
    }

    /// Where it is, later.
    #[lua(name = "settle")]
    async fn settled(&self, lua: Lua, by: Option<i64>) -> mlua::Result<i64> {
        drop(lua);
        Ok(self.0 + by.unwrap_or(0))
    }

    #[lua(skip)]
    fn hidden(&self) {}
}

#[test]
fn a_method_block_declares_and_registers_from_its_signatures() {
    struct Spec(Vec<String>);
    impl super::Members for Spec {
        fn method(&mut self, name: &str, params: &[(&str, &str)], ret: &str, doc: &str) {
            let params: Vec<String> = params.iter().map(|(n, t)| format!("{n}: {t}")).collect();
            self.0
                .push(format!("{name}({}) -> {ret} // {doc}", params.join(", ")));
        }
        fn function(&mut self, _: &str, _: &[(&str, &str)], _: &str, _: &str) {}
        fn field(&mut self, _: &str, _: &str, _: &str) {}
    }
    let mut spec = Spec(Vec::new());
    Valve::lua(&mut spec);
    assert_eq!(
        spec.0,
        [
            "opening() -> number // How far it is open.",
            "turn(name: string, opts: { by: number?, slowly: bool? }?) -> Result<number, string> // Turn it, and say where it is now.",
            "settle(by: number?) -> number // Where it is, later.",
        ]
    );
    Valve(1).hidden();

    let lua = Lua::new();
    lua.globals().set("valve", Valve(10)).expect("set");
    let run = |code: &str| {
        lua.load(code)
            .eval::<(Option<i64>, Option<String>)>()
            .expect("eval")
    };
    assert_eq!(run("return valve:opening()"), (Some(10), None));
    assert_eq!(run("return valve:turn('a')"), (Some(12), None));
    assert_eq!(
        run("return valve:turn('a', { by = 3, slowly = true })"),
        (Some(13), None)
    );
    // the method's own error, and a refused option, are both returned under the method's name
    assert_eq!(
        run("return valve:turn('')"),
        (None, Some("typed.turn: invalid_input: no name".to_owned()))
    );
    assert_eq!(
        run("return valve:turn('a', { fast = true })"),
        (
            None,
            Some("typed.turn: invalid_input: unknown option \"fast\" (this function takes: by, slowly)".to_owned())
        )
    );
    // a plain argument of the wrong Lua type is raised, as for a closure
    assert!(lua.load("return valve:turn({})").exec().is_err());
}
