//! Emit-stage tests. Two layers:
//!
//! 1. **Hand-built AST → Lua.** These construct the AST directly (no parser)
//!    and assert the emitted Lua. An emitter bug shows up here even if the
//!    parser is wrong — the two stages are tested in isolation.
//! 2. **Source → Lua golden.** These parse real source and assert the Lua, to
//!    catch integration mistakes between the stages.

use neoc_ast as ast;
use neoc_codegen_lua::emit;

fn lua(module: &ast::Module) -> String {
    let result = emit(module);
    assert!(
        result.diagnostics.is_empty(),
        "unexpected emit diagnostics: {:#?}",
        result.diagnostics
    );
    result.lua
}

fn lua_from_source(src: &str) -> String {
    let parsed = neoc_parser::parse(src);
    assert!(parsed.ok(), "parse diagnostics: {:#?}", parsed.diagnostics);
    lua(&parsed.module)
}

/// Wraps an [`ast::ExprKind`] in an [`ast::Expr`] with a default span, for the
/// hand-built ASTs these tests construct.
fn e(kind: ast::ExprKind) -> ast::Expr {
    ast::Expr::new(kind, ast::Span::default())
}

// ---- hand-built AST -> Lua (parser-independent) ----------------------------

#[test]
fn emits_a_function_with_a_value_body() {
    // fn add(a: number, b: number) -> number { a + b }
    let num = ast::Type::Path(ast::TypePath {
        segments: vec![ast::TypeSegment {
            name: "number".into(),
            args: vec![],
        }],
    });
    let module = ast::Module {
        items: vec![ast::Item {
            attrs: vec![],
            vis: ast::Visibility::Private,
            kind: ast::ItemKind::Fn(ast::FnDecl {
                attrs: vec![],
                sig: ast::FnSig {
                    name: "add".into(),
                    name_span: ast::Span::default(),
                    generics: vec![],
                    params: vec![
                        ast::Param::Named {
                            name: "a".into(),
                            ty: num.clone(),
                            span: ast::Span::default(),
                        },
                        ast::Param::Named {
                            name: "b".into(),
                            ty: num.clone(),
                            span: ast::Span::default(),
                        },
                    ],
                    ret: Some(num),
                    where_clause: vec![],
                },
                body: ast::Block {
                    stmts: vec![],
                    value: Some(Box::new(e(ast::ExprKind::Binary {
                        op: ast::BinOp::Add,
                        lhs: Box::new(e(ast::ExprKind::Path(vec!["a".into()]))),
                        rhs: Box::new(e(ast::ExprKind::Path(vec!["b".into()]))),
                    }))),
                },
            }),
            span: ast::Span::default(),
        }],
    };
    assert_eq!(
        lua(&module),
        "local function add(a, b)\n  return (a + b)\nend\n"
    );
}

#[test]
fn emits_a_pub_unit_struct() {
    let module = ast::Module {
        items: vec![ast::Item {
            attrs: vec![],
            vis: ast::Visibility::Public,
            kind: ast::ItemKind::Struct(ast::StructDecl {
                name: "Marker".into(),
                generics: vec![],
                body: ast::StructBody::Unit,
            }),
            span: ast::Span::default(),
        }],
    };
    assert_eq!(lua(&module), "Marker = { __struct = \"Marker\" }\n");
}

/// A module with a single `enum E { … }` declaration and a `fn` whose body is
/// `expr`. Returns the emitted Lua. Used to exercise the type-aware lowerings
/// that need the enum in scope.
fn enum_and_fn(variants: Vec<ast::Variant>, expr: ast::Expr) -> ast::Module {
    ast::Module {
        items: vec![
            ast::Item {
                attrs: vec![],
                vis: ast::Visibility::Private,
                kind: ast::ItemKind::Enum(ast::EnumDecl {
                    name: "Shape".into(),
                    generics: vec![],
                    variants,
                }),
                span: ast::Span::default(),
            },
            ast::Item {
                attrs: vec![],
                vis: ast::Visibility::Private,
                kind: ast::ItemKind::Fn(ast::FnDecl {
                    attrs: vec![],
                    sig: ast::FnSig {
                        name: "make".into(),
                        name_span: ast::Span::default(),
                        generics: vec![],
                        params: vec![],
                        ret: None,
                        where_clause: vec![],
                    },
                    body: ast::Block {
                        stmts: vec![],
                        value: Some(Box::new(expr)),
                    },
                }),
                span: ast::Span::default(),
            },
        ],
    }
}

fn variant(name: &str, body: ast::VariantBody) -> ast::Variant {
    ast::Variant {
        attrs: vec![],
        name: name.into(),
        name_span: ast::Span::default(),
        body,
    }
}

#[test]
fn emits_a_unit_enum_variant() {
    // enum Shape { Origin } ; fn make() { Shape::Origin }
    let module = enum_and_fn(
        vec![variant("Origin", ast::VariantBody::Unit)],
        e(ast::ExprKind::Path(vec!["Shape".into(), "Origin".into()])),
    );
    let expected = "\
local function make()
  return { __enum = \"Shape\", __tag = \"Origin\" }
end
";
    assert_eq!(lua(&module), expected);
}

#[test]
fn emits_a_tuple_enum_variant() {
    // enum Shape { Move(number, number) } ; fn make() { Shape::Move(1, 2) }
    let module = enum_and_fn(
        vec![variant(
            "Move",
            ast::VariantBody::Tuple(vec![]), // body types are irrelevant to emit
        )],
        e(ast::ExprKind::Call {
            callee: Box::new(e(ast::ExprKind::Path(vec!["Shape".into(), "Move".into()]))),
            args: vec![
                e(ast::ExprKind::Int("1".into())),
                e(ast::ExprKind::Int("2".into())),
            ],
        }),
    );
    let expected = "\
local function make()
  return { __enum = \"Shape\", __tag = \"Move\", 1, 2 }
end
";
    assert_eq!(lua(&module), expected);
}

#[test]
fn emits_a_struct_enum_variant() {
    // enum Shape { Circle { radius: number } } ; fn make() { Shape::Circle { radius: 3 } }
    let module = enum_and_fn(
        vec![variant("Circle", ast::VariantBody::Named(vec![]))],
        e(ast::ExprKind::StructLit {
            path: vec!["Shape".into(), "Circle".into()],
            fields: vec![ast::FieldInit {
                name: "radius".into(),
                value: Some(e(ast::ExprKind::Int("3".into()))),
            }],
            base: None,
        }),
    );
    let expected = "\
local function make()
  return { __enum = \"Shape\", __tag = \"Circle\", radius = 3 }
end
";
    assert_eq!(lua(&module), expected);
}

#[test]
fn emits_a_method_call_as_a_colon_call() {
    // fn run(p: Point) { p.translate(1, 2) }  (as a statement)
    let module = ast::Module {
        items: vec![ast::Item {
            attrs: vec![],
            vis: ast::Visibility::Private,
            kind: ast::ItemKind::Fn(ast::FnDecl {
                attrs: vec![],
                sig: ast::FnSig {
                    name: "run".into(),
                    name_span: ast::Span::default(),
                    generics: vec![],
                    params: vec![],
                    ret: None,
                    where_clause: vec![],
                },
                body: ast::Block {
                    stmts: vec![ast::Stmt::new(
                        ast::StmtKind::Expr(e(ast::ExprKind::MethodCall {
                            receiver: Box::new(e(ast::ExprKind::Path(vec!["p".into()]))),
                            method: "translate".into(),
                            method_span: ast::Span::default(),
                            args: vec![
                                e(ast::ExprKind::Int("1".into())),
                                e(ast::ExprKind::Int("2".into())),
                            ],
                        })),
                        ast::Span::default(),
                    )],
                    value: None,
                },
            }),
            span: ast::Span::default(),
        }],
    };
    let expected = "\
local function run()
  p:translate(1, 2)
end
";
    assert_eq!(lua(&module), expected);
}

// ---- source -> Lua golden --------------------------------------------------

#[test]
fn golden_factorial() {
    let out = lua_from_source(
        "fn factorial(n: number) -> number {\n\
           if n <= 1 { 1 } else { n * factorial(n - 1) }\n\
         }",
    );
    let expected = "\
local function factorial(n)
  return (function()
    if (n <= 1) then
      return 1
    else
      return (n * factorial((n - 1)))
    end
  end)()
end
";
    assert_eq!(out, expected);
}

#[test]
fn golden_named_struct_and_field_access() {
    let out = lua_from_source(
        "struct Point { x: number, y: number }\n\
         fn shift(p: Point) -> number { let q = Point { x: p.x, y: p.y }; q.x }",
    );
    let expected = "\
local function __type_error(path, expected, got)
  local actual = type(got) == \"table\" and (got.__struct or got.__enum or \"table\") or type(got)
  error(path .. \": expected \" .. expected .. \", got \" .. actual)
end
local function __check_number(v, path) if type(v) ~= \"number\" then __type_error(path, \"number\", v) end return v end
local __check_Point
function __check_Point(v, path)
  if type(v) ~= \"table\" or v.__struct ~= \"Point\" then __type_error(path, \"Point\", v) end
  __check_number(v.x, path .. \".x\")
  __check_number(v.y, path .. \".y\")
  return v
end
local Point = {}
Point.__index = Point
function Point.new(fields)
  fields.__struct = \"Point\"
  return __check_Point(setmetatable(fields, Point), \"Point\")
end
local function shift(p)
  local q = Point.new({ x = p.x, y = p.y })
  return q.x
end
";
    assert_eq!(out, expected);
}

#[test]
fn golden_while_and_for() {
    let out = lua_from_source(
        "fn run(n: number) {\n\
           let mut i = 0;\n\
           while i < n { i = i; }\n\
           for k in 0..n { step(k); }\n\
         }",
    );
    let expected = "\
local function run(n)
  local i = 0
  while (i < n) do
    i = i
  end
  for k = 0, (n) - 1 do
    step(k)
  end
end
";
    assert_eq!(out, expected);
}

#[test]
fn golden_match_over_enum_variants() {
    let out = lua_from_source(
        "enum Shape { Circle { radius: number }, Move(number, number), Origin }\n\
         fn area(s: Shape) -> number {\n\
           match s {\n\
             Shape::Circle { radius: r } => r,\n\
             Shape::Move(x, y) => x,\n\
             Shape::Origin => 0,\n\
           }\n\
         }",
    );
    let expected = "\
local function area(s)
  return (function()
    local __s = s
    if __s.__tag == \"Circle\" then
      local r = __s.radius
      return r
    end
    if __s.__tag == \"Move\" then
      local x = __s[1]
      local y = __s[2]
      return x
    end
    if __s.__tag == \"Origin\" then
      return 0
    end
  end)()
end
";
    assert_eq!(out, expected);
}

#[test]
fn golden_match_literals_or_guard_wildcard() {
    let out = lua_from_source(
        "fn name(n: number) -> string {\n\
           match n { 0 => \"z\", 1 | 2 => \"s\", x if x > 9 => \"big\", _ => \"o\" }\n\
         }",
    );
    let expected = "\
local function name(n)
  return (function()
    local __s = n
    if __s == 0 then
      return \"z\"
    end
    if ((__s == 1) or (__s == 2)) then
      return \"s\"
    end
    if true then
      local x = __s
      if (x > 9) then
        return \"big\"
      end
    end
    if true then
      return \"o\"
    end
  end)()
end
";
    assert_eq!(out, expected);
}

#[test]
fn golden_continue_uses_repeat_break_idiom() {
    // Lua 5.1 has no `continue`; a loop body that uses it is restructured with
    // the repeat/break idiom. `continue` becomes a `break` of the `repeat`; a
    // neoc `break` sets the outer flag and breaks, exiting the loop via the
    // `if <flag> then break end` after the `repeat` (DYNAMIC.md §8).
    let out = lua_from_source(
        "fn run(n: number) {\n\
           for i in 0..n {\n\
             if i == 3 { continue }\n\
             if i == 7 { break }\n\
             step(i);\n\
           }\n\
         }",
    );
    let expected = "\
local function run(n)
  for i = 0, (n) - 1 do
    local __brk_1 = false
    repeat
      if (i == 3) then
        break
      end
      if (i == 7) then
        do __brk_1 = true; break end
      end
      step(i)
    until true
    if __brk_1 then break end
  end
end
";
    assert_eq!(out, expected);
}

#[test]
fn golden_loop_without_continue_stays_plain() {
    // A loop with no `continue` keeps a plain body — `break` is bare Lua.
    let out = lua_from_source(
        "fn run(n: number) {\n\
           while i < n { if i == 3 { break } step(i); }\n\
         }",
    );
    let expected = "\
local function run(n)
  while (i < n) do
    if (i == 3) then
      break
    end
    step(i)
  end
end
";
    assert_eq!(out, expected);
}

#[test]
fn golden_let_struct_destructure() {
    let out = lua_from_source(
        "struct Point { x: number, y: number }\n\
         fn sum(p: Point) -> number { let Point { x, y } = p; x + y }",
    );
    let expected = "\
local function __type_error(path, expected, got)
  local actual = type(got) == \"table\" and (got.__struct or got.__enum or \"table\") or type(got)
  error(path .. \": expected \" .. expected .. \", got \" .. actual)
end
local function __check_number(v, path) if type(v) ~= \"number\" then __type_error(path, \"number\", v) end return v end
local __check_Point
function __check_Point(v, path)
  if type(v) ~= \"table\" or v.__struct ~= \"Point\" then __type_error(path, \"Point\", v) end
  __check_number(v.x, path .. \".x\")
  __check_number(v.y, path .. \".y\")
  return v
end
local Point = {}
Point.__index = Point
function Point.new(fields)
  fields.__struct = \"Point\"
  return __check_Point(setmetatable(fields, Point), \"Point\")
end
local function sum(p)
  local __d = p
  local x = __d.x
  local y = __d.y
  return (x + y)
end
";
    assert_eq!(out, expected);
}

#[test]
fn golden_newtype_construction_routes_through_new() {
    // Calling a tuple/newtype struct constructs through `.new` (§4.4), so the
    // value carries its `__struct` tag and metatable — not a bare Lua call.
    let out = lua_from_source(
        "struct Wrapper(number);\n\
         fn wrap() -> Wrapper { Wrapper(42) }",
    );
    let expected = "\
local function __type_error(path, expected, got)
  local actual = type(got) == \"table\" and (got.__struct or got.__enum or \"table\") or type(got)
  error(path .. \": expected \" .. expected .. \", got \" .. actual)
end
local function __check_number(v, path) if type(v) ~= \"number\" then __type_error(path, \"number\", v) end return v end
local __check_Wrapper
function __check_Wrapper(v, path)
  if type(v) ~= \"table\" or v.__struct ~= \"Wrapper\" then __type_error(path, \"Wrapper\", v) end
  __check_number(v[1], path .. \".0\")
  return v
end
local Wrapper = {}
Wrapper.__index = Wrapper
function Wrapper.new(value)
  return __check_Wrapper(setmetatable({ __struct = \"Wrapper\", value }, Wrapper), \"Wrapper\")
end
local function wrap()
  return Wrapper.new(42)
end
";
    assert_eq!(out, expected);
}

#[test]
fn golden_functional_struct_update() {
    let out = lua_from_source(
        "struct Point { x: number, y: number }\n\
         fn shift(p: Point) -> Point { Point { x: 9, ..p } }",
    );
    let expected = "\
local function __type_error(path, expected, got)
  local actual = type(got) == \"table\" and (got.__struct or got.__enum or \"table\") or type(got)
  error(path .. \": expected \" .. expected .. \", got \" .. actual)
end
local function __check_number(v, path) if type(v) ~= \"number\" then __type_error(path, \"number\", v) end return v end
local __check_Point
function __check_Point(v, path)
  if type(v) ~= \"table\" or v.__struct ~= \"Point\" then __type_error(path, \"Point\", v) end
  __check_number(v.x, path .. \".x\")
  __check_number(v.y, path .. \".y\")
  return v
end
local Point = {}
Point.__index = Point
function Point.new(fields)
  fields.__struct = \"Point\"
  return __check_Point(setmetatable(fields, Point), \"Point\")
end
local function shift(p)
  return Point.new((function()
    local __b = {}
    for k, v in pairs(p) do __b[k] = v end
    __b.x = 9
    return __b
  end)())
end
";
    assert_eq!(out, expected);
}

// ---- runtime type enforcement (DYNAMIC.md §9, STATIC.md §10) ----------------

#[test]
fn validators_check_pub_fn_params_but_not_private() {
    // A `pub` free function is host-reachable, so each typed parameter is
    // validated at entry (DRIFT-PARAM); a private function's are vouched
    // (STABLE-PARAM) and emit no check.
    let out = lua_from_source(
        "pub fn area(w: number, label: string) -> number { w }\n\
         fn helper(n: number) -> number { n }",
    );
    let expected = "\
local function __type_error(path, expected, got)
  local actual = type(got) == \"table\" and (got.__struct or got.__enum or \"table\") or type(got)
  error(path .. \": expected \" .. expected .. \", got \" .. actual)
end
local function __check_number(v, path) if type(v) ~= \"number\" then __type_error(path, \"number\", v) end return v end
local function __check_string(v, path) if type(v) ~= \"string\" then __type_error(path, \"string\", v) end return v end
function area(w, label)
  __check_number(w, \"area.w\")
  __check_string(label, \"area.label\")
  return w
end
local function helper(n)
  return n
end
";
    assert_eq!(out, expected);
}

#[test]
fn validators_dispatch_enum_variants_and_recurse_into_fields() {
    // An enum validator checks the tag, then dispatches on the variant and
    // validates that variant's payload — tuple slots positionally, struct
    // fields by name, a unit variant carrying nothing. A struct field of the
    // enum type recurses into it (deep), the two validators mutually
    // forward-declared so either order resolves.
    let out = lua_from_source(
        "enum Shape { Dot, Pair(number, number), Named { side: number } }\n\
         struct Holder { s: Shape }",
    );
    let expected = "\
local function __type_error(path, expected, got)
  local actual = type(got) == \"table\" and (got.__struct or got.__enum or \"table\") or type(got)
  error(path .. \": expected \" .. expected .. \", got \" .. actual)
end
local function __check_number(v, path) if type(v) ~= \"number\" then __type_error(path, \"number\", v) end return v end
local __check_Holder, __check_Shape
function __check_Holder(v, path)
  if type(v) ~= \"table\" or v.__struct ~= \"Holder\" then __type_error(path, \"Holder\", v) end
  __check_Shape(v.s, path .. \".s\")
  return v
end
function __check_Shape(v, path)
  if type(v) ~= \"table\" or v.__enum ~= \"Shape\" then __type_error(path, \"Shape\", v) end
  local t = v.__tag
  if t == \"Dot\" then
  elseif t == \"Pair\" then
    __check_number(v[1], path .. \".0\")
    __check_number(v[2], path .. \".1\")
  elseif t == \"Named\" then
    __check_number(v.side, path .. \".side\")
  else __type_error(path, \"Shape\", v) end
  return v
end
local Holder = {}
Holder.__index = Holder
function Holder.new(fields)
  fields.__struct = \"Holder\"
  return __check_Holder(setmetatable(fields, Holder), \"Holder\")
end
";
    assert_eq!(out, expected);
}

#[test]
fn validators_compose_list_option_and_result_combinators() {
    // A structural field type validates through a combinator that takes its
    // element/payload validator as an argument, so it composes without
    // inlining (DYNAMIC.md §9.1).
    let out = lua_from_source(
        "struct Bag { items: [number], tag: Option<string>, outcome: Result<number, string> }",
    );
    assert!(out.contains("__check_list(v.items, path .. \".items\", __check_number)"));
    assert!(out.contains("__check_option(v.tag, path .. \".tag\", __check_string)"));
    assert!(out.contains(
        "__check_result(v.outcome, path .. \".outcome\", __check_number, __check_string)"
    ));
    // Each combinator it composes is emitted once.
    assert!(out.contains("local function __check_list(v, path, elem)"));
    assert!(out.contains("local function __check_option(v, path, some)"));
    assert!(out.contains("local function __check_result(v, path, ok, err)"));
}

#[test]
fn validators_nest_list_combinators_for_a_list_of_lists() {
    // A `[[number]]` field nests the list combinator: the outer call's element
    // validator is itself a `__check_list` closure over the number checker, so
    // composition recurses without inlining.
    let out = lua_from_source("struct Grid { rows: [[number]] }");
    assert!(out.contains("__check_list(v.rows, path .. \".rows\", function(v, p) return __check_list(v, p, __check_number) end)"));
}

#[test]
fn validators_omitted_when_no_drift_point_exists() {
    // No struct, enum, or `pub fn` — nothing to enforce, so no preamble at all.
    let out = lua_from_source("fn helper(n: number) -> number { n }");
    assert!(!out.contains("__type_error"));
    assert!(!out.contains("__check_"));
}

#[test]
fn validators_skip_imported_and_type_parameter_fields() {
    // A field whose type the module cannot inspect — a bare type parameter —
    // degrades to a presence-only check (the tag), never a dangling reference
    // to a validator this module does not own.
    let out = lua_from_source("struct Box<T> { value: T }");
    assert!(out.contains("function __check_Box(v, path)"));
    assert!(out.contains("if type(v) ~= \"table\" or v.__struct ~= \"Box\""));
    assert!(!out.contains("__check_T"));
    assert!(!out.contains("v.value")); // the `T` field carries no check
}

#[test]
fn golden_enum_with_methods_gets_a_metatable() {
    // An enum with an `impl` block emits a method table; each variant value is
    // `setmetatable`d so `:method` dispatch resolves (DYNAMIC.md §3.2).
    let out = lua_from_source(
        "enum Dir { N, S }\n\
         impl Dir { fn flip(self) -> Dir { Dir::N } }\n\
         fn make() -> Dir { Dir::S }",
    );
    let expected = "\
local Dir = {}
Dir.__index = Dir
function Dir.flip(self)
  return setmetatable({ __enum = \"Dir\", __tag = \"N\" }, Dir)
end
local function make()
  return setmetatable({ __enum = \"Dir\", __tag = \"S\" }, Dir)
end
";
    assert_eq!(out, expected);
}

#[test]
fn golden_try_operator() {
    let out = lua_from_source(
        "fn run(r: Result<number, string>) -> Result<number, string> {\n\
           let v = parse()?;\n\
           Ok(v)\n\
         }",
    );
    let expected = "\
local Ok   = function(v) return { __enum = \"Result\", __tag = \"Ok\", v } end
local Err  = function(e) return { __enum = \"Result\", __tag = \"Err\", e } end
local Some = function(v) return { __enum = \"Option\", __tag = \"Some\", v } end
local None = { __enum = \"Option\", __tag = \"None\" }

local function run(r)
  local __try_0 = parse()
  if __try_0.__tag == \"Err\" or __try_0.__tag == \"None\" then return __try_0 end
  local v = __try_0[1]
  return Ok(v)
end
";
    assert_eq!(out, expected);
}

// ---- source map (#55 / #56) -------------------------------------------------

use neoc_codegen_lua::emit_with_source;

/// The neoc line a one-based emitted-Lua line maps back to.
fn neoc_line_of(src: &str, lua_line: u32) -> Option<u32> {
    let parsed = neoc_parser::parse(src);
    assert!(parsed.ok(), "parse diagnostics: {:#?}", parsed.diagnostics);
    let result = emit_with_source(&parsed.module, src);
    assert!(
        result.diagnostics.is_empty(),
        "unexpected emit diagnostics: {:#?}",
        result.diagnostics
    );
    result.source_map.neoc_line(lua_line)
}

#[test]
fn source_map_points_emitted_lines_back_to_neoc_lines() {
    // neoc line 1: fn main()
    // neoc line 2:   let x = boom();   <- the offending call
    // neoc line 3: }
    let src = "fn main() {\n  let x = boom();\n}\n";
    // Emitted Lua:
    //   1: local function main()
    //   2:   local x = boom()
    //   3: end
    assert_eq!(
        neoc_line_of(src, 1),
        Some(1),
        "fn header maps to neoc line 1"
    );
    assert_eq!(
        neoc_line_of(src, 2),
        Some(2),
        "the `boom()` call maps to neoc line 2"
    );
}

#[test]
fn source_map_handles_a_later_statement() {
    // The body statement sits on neoc line 4.
    let src = "fn main() {\n\n\n  go();\n}\n";
    // Lua line 2 is `  go()`.
    assert_eq!(neoc_line_of(src, 2), Some(4));
}

#[test]
fn source_map_empty_without_source() {
    let parsed = neoc_parser::parse("fn main() {\n  go();\n}\n");
    let result = emit(&parsed.module);
    assert_eq!(result.source_map, neoc_codegen_lua::LineMap::default());
    assert_eq!(result.source_map.neoc_line(1), None);
}

// ---- forward-referenced private functions ----------------------------------

#[test]
fn forward_referenced_private_fn_is_hoisted() {
    // `main` calls `helper`, defined after it. `helper` must be declared `local`
    // up front and assigned in place, so `main` closes over the local, not a nil
    // global — a plain `local function helper` would be invisible to `main`.
    let lua = lua_from_source("fn main() -> number { helper() }\nfn helper() -> number { 1 }\n");
    let hoist = lua.find("local helper").expect("forward declaration");
    let caller = lua.find("local function main").expect("caller");
    assert!(hoist < caller, "hoist must precede the caller\n{lua}");
    assert!(
        lua.contains("helper = function"),
        "assigned in place\n{lua}"
    );
}

#[test]
fn a_fn_defined_before_use_is_not_hoisted() {
    // `helper` precedes its use — an ordinary `local function`, no hoist.
    let lua = lua_from_source("fn helper() -> number { 1 }\nfn main() -> number { helper() }\n");
    assert!(lua.contains("local function helper"), "{lua}");
    assert!(
        !lua.lines().any(|l| l.trim() == "local helper"),
        "no forward declaration expected\n{lua}"
    );
}

#[test]
fn associated_fn_call_keeps_the_type_qualifier() {
    // `Counter::zero()` is an associated function on `Counter`'s table; it must
    // emit `Counter.zero()`, not a bare `zero()` (an unbound global). The impl
    // target makes `Counter` a known associated-item carrier.
    let lua = lua_from_source(
        "struct Counter { n: number }\nimpl Counter { fn zero() -> Counter { Counter { n: 0 } } }\nfn main() -> Counter { Counter::zero() }\n",
    );
    assert!(
        lua.contains("return Counter.zero()"),
        "associated call keeps the `Counter.` qualifier\n{lua}"
    );
    assert!(
        !lua.lines().any(|l| l.trim() == "return zero()"),
        "the bare `zero()` would reference an unbound global\n{lua}"
    );
}

// ---- emit diagnostics for ill-formed value positions -----------------------
// The type-checker rejects most of these first, but the emitter carries its own
// defensive diagnostics for a node reaching codegen in value position. These
// hand-build that node directly so the branch is exercised in isolation.

/// A private `fn make()` whose block value is `expr`.
fn fn_value_body(expr: ast::Expr) -> ast::Module {
    ast::Module {
        items: vec![ast::Item {
            attrs: vec![],
            vis: ast::Visibility::Private,
            kind: ast::ItemKind::Fn(ast::FnDecl {
                attrs: vec![],
                sig: ast::FnSig {
                    name: "make".into(),
                    name_span: ast::Span::default(),
                    generics: vec![],
                    params: vec![],
                    ret: None,
                    where_clause: vec![],
                },
                body: ast::Block {
                    stmts: vec![],
                    value: Some(Box::new(expr)),
                },
            }),
            span: ast::Span::default(),
        }],
    }
}

fn emit_diagnostics(module: &ast::Module) -> Vec<String> {
    emit(module)
        .diagnostics
        .into_iter()
        .map(|d| d.message)
        .collect()
}

fn assert_value_position_diagnostic(expr: ast::ExprKind, expected: &str) {
    let diags = emit_diagnostics(&fn_value_body(e(expr)));
    assert!(
        diags.iter().any(|m| m == expected),
        "expected emit diagnostic {expected:?}, got {diags:?}"
    );
}

#[test]
fn continue_in_value_position_diagnoses() {
    assert_value_position_diagnostic(
        ast::ExprKind::Continue,
        "control-flow expression used in value position",
    );
}

#[test]
fn break_in_value_position_diagnoses() {
    assert_value_position_diagnostic(
        ast::ExprKind::Break,
        "control-flow expression used in value position",
    );
}

#[test]
fn return_in_value_position_diagnoses() {
    assert_value_position_diagnostic(
        ast::ExprKind::Return(None),
        "control-flow expression used in value position",
    );
}

#[test]
fn loop_in_value_position_diagnoses() {
    assert_value_position_diagnostic(
        ast::ExprKind::Loop {
            body: ast::Block::default(),
        },
        "loops do not produce a value",
    );
}

#[test]
fn while_in_value_position_diagnoses() {
    assert_value_position_diagnostic(
        ast::ExprKind::While {
            cond: Box::new(e(ast::ExprKind::Path(vec!["x".into()]))),
            body: ast::Block::default(),
        },
        "loops do not produce a value",
    );
}

#[test]
fn range_in_value_position_diagnoses() {
    assert_value_position_diagnostic(
        ast::ExprKind::Range {
            start: Box::new(e(ast::ExprKind::Path(vec!["a".into()]))),
            end: Box::new(e(ast::ExprKind::Path(vec!["b".into()]))),
            inclusive: false,
        },
        "range expressions are only supported as a `for` iterable",
    );
}

#[test]
fn assignment_in_value_position_diagnoses() {
    assert_value_position_diagnostic(
        ast::ExprKind::Assign {
            target: Box::new(e(ast::ExprKind::Path(vec!["x".into()]))),
            op: ast::AssignOp::Assign,
            value: Box::new(e(ast::ExprKind::Path(vec!["y".into()]))),
        },
        "assignment is a statement, not an expression value",
    );
}
