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
                    generics: vec![],
                    params: vec![
                        ast::Param::Named {
                            name: "a".into(),
                            ty: num.clone(),
                        },
                        ast::Param::Named {
                            name: "b".into(),
                            ty: num.clone(),
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
    assert_eq!(lua(&module), "Marker = { _struct = \"Marker\" }\n");
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
  return { _enum = \"Shape\", tag = \"Origin\" }
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
  return { _enum = \"Shape\", tag = \"Move\", 1, 2 }
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
  return { _enum = \"Shape\", tag = \"Circle\", radius = 3 }
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
local Point = {}
Point.__index = Point
function Point.new(fields)
  fields._struct = \"Point\"
  return setmetatable(fields, Point)
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
    if __s.tag == \"Circle\" then
      local r = __s.radius
      return r
    end
    if __s.tag == \"Move\" then
      local x = __s[1]
      local y = __s[2]
      return x
    end
    if __s.tag == \"Origin\" then
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
local Point = {}
Point.__index = Point
function Point.new(fields)
  fields._struct = \"Point\"
  return setmetatable(fields, Point)
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
    // value carries its `_struct` tag and metatable — not a bare Lua call.
    let out = lua_from_source(
        "struct Wrapper(number);\n\
         fn wrap() -> Wrapper { Wrapper(42) }",
    );
    let expected = "\
local Wrapper = {}
Wrapper.__index = Wrapper
function Wrapper.new(value)
  return setmetatable({ _struct = \"Wrapper\", value }, Wrapper)
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
local Point = {}
Point.__index = Point
function Point.new(fields)
  fields._struct = \"Point\"
  return setmetatable(fields, Point)
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
  return setmetatable({ _enum = \"Dir\", tag = \"N\" }, Dir)
end
local function make()
  return setmetatable({ _enum = \"Dir\", tag = \"S\" }, Dir)
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
local Ok   = function(v) return { _enum = \"Result\", tag = \"Ok\", v } end
local Err  = function(e) return { _enum = \"Result\", tag = \"Err\", e } end
local Some = function(v) return { _enum = \"Option\", tag = \"Some\", v } end
local None = { _enum = \"Option\", tag = \"None\" }

local function run(r)
  local __try_0 = parse()
  if __try_0.tag == \"Err\" or __try_0.tag == \"None\" then return __try_0 end
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
