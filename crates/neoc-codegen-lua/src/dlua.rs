//! neoc → `LuaLS` definition-file projector. Pure functions mapping a neoc AST
//! type, `FnSig`, `struct`, `enum`, or `const` to `LuaLS` (`luau-lsp`/`lua-language-server`)
//! annotation text for a `.d.lua` sidecar.
//!
//! The sidecar describes the **actual emitted Lua values**: the composite shapes
//! — `struct`, `enum`, `Result`, `Option` — mirror the runtime lowering the Lua
//! backend emits (`lang/DYNAMIC.md` §3.1, §3.2, §7), so a Lua-LSP user can
//! `match` on `__tag` with full narrowing across the boundary. Tuple payloads
//! live at integer keys (`[1]`, `[2]`, …), named fields by name — exactly the
//! tagged tables [`crate::Emitter`] builds.
//!
//! The projection uses only `LuaLS`'s **native** annotation forms — `---@class`,
//! `---@field`, `---@alias`, `---@param`, `---@return` — never an invented
//! helper. These functions are wasm-clean (the crate is in the leaf gate): pure
//! string building over `&ast` nodes, no host facilities.

use std::fmt::Write as _;

use neoc_ast as ast;

/// Project a neoc [`ast::Type`] to its `LuaLS` type form.
///
/// The leaf mappings (mirroring the runtime values the Lua backend emits):
/// `number`/`string` keep their names; `bool` → `boolean`; `()` (unit) → `nil`;
/// `[T]` → `T[]`; `fn(A) -> B` → `fun(a0: A): B`; a generic / user type keeps its
/// name with `LuaLS` type arguments (`Result<number, string>` → `Result<number, string>`).
/// `Self` has no declaration-position meaning in `LuaLS`, so it falls back to the
/// conventional `self`.
#[must_use]
pub fn lua_type(ty: &ast::Type) -> String {
    match ty {
        // A neoc unit value lowers to `nil` at runtime.
        ast::Type::Unit => "nil".to_string(),
        ast::Type::List(inner) => format!("{}[]", lua_type_atom(inner)),
        ast::Type::Fn { params, ret } => {
            let ps = params
                .iter()
                .enumerate()
                .map(|(i, p)| format!("a{i}: {}", lua_type(p)))
                .collect::<Vec<_>>()
                .join(", ");
            let r = ret.as_deref().map_or_else(|| "nil".to_string(), lua_type);
            format!("fun({ps}): {r}")
        }
        ast::Type::SelfType => "self".to_string(),
        ast::Type::Path(p) => lua_path(p),
    }
}

/// Project a type in a position where a postfix `[]` would bind too loosely — a
/// function type must be parenthesised before `[]`.
fn lua_type_atom(ty: &ast::Type) -> String {
    match ty {
        ast::Type::Fn { .. } => format!("({})", lua_type(ty)),
        _ => lua_type(ty),
    }
}

/// Project a neoc type path (`number`, `Result<T, E>`, `Foo<Bar>`) to `LuaLS`.
fn lua_path(p: &ast::TypePath) -> String {
    let Some(seg) = p.segments.last() else {
        return "any".to_string();
    };
    let base = match seg.name.as_str() {
        "number" => "number".to_string(),
        "string" => "string".to_string(),
        "bool" => "boolean".to_string(),
        // A bare opaque foreign type (`ext type H` / an unresolvable name) keeps
        // its name; the consumer sees a nominal handle.
        other => other.to_string(),
    };
    if seg.args.is_empty() {
        base
    } else {
        let args = seg.args.iter().map(lua_type).collect::<Vec<_>>().join(", ");
        format!("{base}<{args}>")
    }
}

/// Render a generic parameter list (`<T, U>`), or empty when there are none.
fn lua_generics(generics: &[ast::GenericParam]) -> String {
    if generics.is_empty() {
        return String::new();
    }
    let ps = generics
        .iter()
        .map(|g| g.name.clone())
        .collect::<Vec<_>>()
        .join(", ");
    format!("<{ps}>")
}

/// Project a `struct` declaration to a `LuaLS` `---@class` with one `---@field`
/// per field, mirroring the emitted tagged table (`lang/DYNAMIC.md` §3.1).
///
/// * Named struct `struct S { a: number }` → `---@class S` + `---@field a number`.
/// * Newtype/tuple `struct N(T)` → `---@class N` + `---@field [1] T`, matching the
///   runtime lowering that stores the payload at integer key `[1]`.
/// * Unit struct `struct S;` → `---@class S` with no payload field.
///
/// Every class carries the `__struct` runtime tag as a string-literal field — the
/// value's runtime identity (`match`/`Eq`/`Debug` read it), so the `.d.lua`
/// describes the actual table. A non-`pub` struct still projects (its annotations
/// are local-scoped by the consumer); `exported` is accepted for API symmetry with
/// the JS projector and to leave room for future visibility shaping.
#[must_use]
pub fn project_struct(s: &ast::StructDecl, exported: bool) -> String {
    let _ = exported;
    let g = lua_generics(&s.generics);
    let mut out = format!("---@class {}{g}\n", s.name);
    let _ = writeln!(out, "---@field __struct \"{}\"", s.name);
    match &s.body {
        ast::StructBody::Unit => {}
        ast::StructBody::Tuple(t) => {
            let _ = writeln!(out, "---@field [1] {}", lua_type(t));
        }
        ast::StructBody::Named(fields) => {
            for f in fields {
                let _ = writeln!(out, "---@field {} {}", f.name, lua_type(&f.ty));
            }
        }
    }
    out
}

/// Project an `enum` declaration to a `LuaLS` class-union: one `---@class` per
/// variant whose fields match the runtime lowering the Lua backend emits
/// (`lang/DYNAMIC.md` §3.2), unified by an `---@alias`.
///
/// * Unit variant `V` → `---@class E.V` with `__enum`/`__tag` literal fields.
/// * Tuple variant `V(A, B)` → `---@class E.V` + `---@field [1] A` + `---@field [2] B`.
/// * Struct variant `V { x: T }` → `---@class E.V` + `---@field x T`.
///
/// The union is `---@alias E E.A | E.B | …`. Carrying `__enum`/`__tag` as
/// string-literal fields is what lets a Lua-LSP user narrow on `__tag` — the same
/// discriminant a `match` reads — so it round-trips across the boundary.
#[must_use]
pub fn project_enum(e: &ast::EnumDecl, exported: bool) -> String {
    let _ = exported;
    let mut out = String::new();
    let mut member_names = Vec::with_capacity(e.variants.len());
    for v in &e.variants {
        out.push_str(&enum_variant_class(&e.name, v));
        out.push('\n');
        member_names.push(format!("{}.{}", e.name, v.name));
    }
    let g = lua_generics(&e.generics);
    let union = if member_names.is_empty() {
        // An empty enum has no inhabitant; `LuaLS` spells the bottom as `nil`.
        "nil".to_string()
    } else {
        member_names.join(" | ")
    };
    let _ = write!(out, "---@alias {}{g} {union}", e.name);
    out
}

/// One member class of an enum's union, mirroring the emitted Lua value shape for
/// that variant. The discriminant fields (`__enum`, `__tag`) are string literals
/// so a consumer narrows on them.
fn enum_variant_class(enum_name: &str, v: &ast::Variant) -> String {
    let mut s = format!("---@class {enum_name}.{}\n", v.name);
    let _ = writeln!(s, "---@field __enum \"{enum_name}\"");
    let _ = write!(s, "---@field __tag \"{}\"", v.name);
    match &v.body {
        ast::VariantBody::Unit => {}
        ast::VariantBody::Tuple(tys) => {
            for (i, t) in tys.iter().enumerate() {
                // Tuple payload lives at integer keys `[1]`, `[2]`, … (§3.2).
                let _ = write!(s, "\n---@field [{}] {}", i + 1, lua_type(t));
            }
        }
        ast::VariantBody::Named(fields) => {
            for f in fields {
                let _ = write!(s, "\n---@field {} {}", f.name, lua_type(&f.ty));
            }
        }
    }
    s
}

/// Project a function signature to an annotated `LuaLS` function stub: a
/// `---@param` per named parameter, a `---@return`, and a `function M.f(a) end`
/// body returning the module table member.
///
/// The `self` receiver (a method) is dropped — methods are not free module
/// members; only free `pub fn`s reach this. Each named parameter contributes a
/// `---@param name type` and its bare name to the stub's parameter list; the
/// return type projects via [`lua_type`] (absent → `nil`).
#[must_use]
pub fn project_fn(sig: &ast::FnSig, exported: bool) -> String {
    let _ = exported;
    let mut names = Vec::new();
    let mut anns = String::new();
    if !sig.generics.is_empty() {
        let _ = writeln!(
            anns,
            "---@generic {}",
            sig.generics
                .iter()
                .map(|g| g.name.clone())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    for p in &sig.params {
        if let ast::Param::Named { name, ty, .. } = p {
            let _ = writeln!(anns, "---@param {name} {}", lua_type(ty));
            names.push(name.clone());
        }
    }
    let ret = sig.ret.as_ref().map_or_else(|| "nil".to_string(), lua_type);
    let _ = writeln!(anns, "---@return {ret}");
    format!("{anns}function M.{}({}) end", sig.name, names.join(", "))
}

/// Project a `const` declaration to a `LuaLS`-annotated module field. neoc consts
/// are typed (`const NAME: T = …`), so the declared type projects as a
/// `---@type`; the value is elided (a `.d.lua` carries no initializer).
#[must_use]
pub fn project_const(c: &ast::ConstDecl, exported: bool) -> String {
    let _ = exported;
    format!("---@type {}\nM.{} = nil", lua_type(&c.ty), c.name)
}

/// Project a `type` alias to a `LuaLS` `---@alias`.
#[must_use]
pub fn project_type_alias(a: &ast::TypeAlias, exported: bool) -> String {
    let _ = exported;
    let g = lua_generics(&a.generics);
    format!("---@alias {}{g} {}", a.name, lua_type(&a.value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(name: &str) -> ast::Type {
        ast::Type::Path(ast::TypePath {
            segments: vec![ast::TypeSegment {
                name: name.to_string(),
                args: Vec::new(),
            }],
        })
    }

    fn generic(name: &str, args: Vec<ast::Type>) -> ast::Type {
        ast::Type::Path(ast::TypePath {
            segments: vec![ast::TypeSegment {
                name: name.to_string(),
                args,
            }],
        })
    }

    fn field(name: &str, ty: ast::Type) -> ast::Field {
        ast::Field {
            attrs: Vec::new(),
            name: name.to_string(),
            name_span: ast::Span::default(),
            ty,
        }
    }

    #[test]
    fn primitives_and_unit() {
        assert_eq!(lua_type(&path("number")), "number");
        assert_eq!(lua_type(&path("string")), "string");
        assert_eq!(lua_type(&path("bool")), "boolean");
        assert_eq!(lua_type(&ast::Type::Unit), "nil");
    }

    #[test]
    fn list_and_fn_and_generics() {
        assert_eq!(
            lua_type(&ast::Type::List(Box::new(path("number")))),
            "number[]"
        );
        let f = ast::Type::Fn {
            params: vec![path("number")],
            ret: Some(Box::new(path("string"))),
        };
        assert_eq!(lua_type(&f), "fun(a0: number): string");
        // A list of functions parenthesises the element type.
        let lf = ast::Type::List(Box::new(ast::Type::Fn {
            params: vec![],
            ret: None,
        }));
        assert_eq!(lua_type(&lf), "(fun(): nil)[]");
        assert_eq!(
            lua_type(&generic("Result", vec![path("number"), path("string")])),
            "Result<number, string>"
        );
    }

    #[test]
    fn struct_named_class_with_fields() {
        let s = ast::StructDecl {
            name: "S".to_string(),
            generics: Vec::new(),
            body: ast::StructBody::Named(vec![field("a", path("number"))]),
        };
        assert_eq!(
            project_struct(&s, true),
            "---@class S\n---@field __struct \"S\"\n---@field a number\n"
        );
    }

    #[test]
    fn struct_newtype_payload_at_integer_one() {
        let s = ast::StructDecl {
            name: "N".to_string(),
            generics: Vec::new(),
            body: ast::StructBody::Tuple(path("number")),
        };
        assert_eq!(
            project_struct(&s, true),
            "---@class N\n---@field __struct \"N\"\n---@field [1] number\n"
        );
    }

    #[test]
    fn struct_unit_class_no_payload() {
        let s = ast::StructDecl {
            name: "U".to_string(),
            generics: Vec::new(),
            body: ast::StructBody::Unit,
        };
        assert_eq!(
            project_struct(&s, true),
            "---@class U\n---@field __struct \"U\"\n"
        );
    }

    #[test]
    fn enum_class_union_matches_runtime_tags() {
        // enum E { A, B(number), C { x: string } }
        let e = ast::EnumDecl {
            name: "E".to_string(),
            generics: Vec::new(),
            variants: vec![
                ast::Variant {
                    attrs: Vec::new(),
                    name: "A".to_string(),
                    name_span: ast::Span::default(),
                    body: ast::VariantBody::Unit,
                },
                ast::Variant {
                    attrs: Vec::new(),
                    name: "B".to_string(),
                    name_span: ast::Span::default(),
                    body: ast::VariantBody::Tuple(vec![path("number")]),
                },
                ast::Variant {
                    attrs: Vec::new(),
                    name: "C".to_string(),
                    name_span: ast::Span::default(),
                    body: ast::VariantBody::Named(vec![field("x", path("string"))]),
                },
            ],
        };
        assert_eq!(
            project_enum(&e, true),
            "---@class E.A\n\
             ---@field __enum \"E\"\n\
             ---@field __tag \"A\"\n\
             ---@class E.B\n\
             ---@field __enum \"E\"\n\
             ---@field __tag \"B\"\n\
             ---@field [1] number\n\
             ---@class E.C\n\
             ---@field __enum \"E\"\n\
             ---@field __tag \"C\"\n\
             ---@field x string\n\
             ---@alias E E.A | E.B | E.C"
        );
    }

    #[test]
    fn fn_stub_with_param_and_result_return() {
        // fn find(id: number) -> Result<string, string>
        let sig = ast::FnSig {
            name: "find".to_string(),
            name_span: ast::Span::default(),
            generics: Vec::new(),
            params: vec![ast::Param::Named {
                name: "id".to_string(),
                ty: path("number"),
                span: ast::Span::default(),
            }],
            ret: Some(generic("Result", vec![path("string"), path("string")])),
            where_clause: Vec::new(),
        };
        assert_eq!(
            project_fn(&sig, true),
            "---@param id number\n\
             ---@return Result<string, string>\n\
             function M.find(id) end"
        );
    }

    #[test]
    fn fn_stub_drops_receiver_and_nils_absent_return() {
        let sig = ast::FnSig {
            name: "tick".to_string(),
            name_span: ast::Span::default(),
            generics: Vec::new(),
            params: vec![ast::Param::Receiver],
            ret: None,
            where_clause: Vec::new(),
        };
        assert_eq!(
            project_fn(&sig, true),
            "---@return nil\nfunction M.tick() end"
        );
    }

    #[test]
    fn const_projects_typed_module_field() {
        let c = ast::ConstDecl {
            name: "MAX".to_string(),
            ty: path("number"),
            value: ast::Expr::new(ast::ExprKind::Int("9".to_string()), ast::Span::default()),
        };
        assert_eq!(project_const(&c, true), "---@type number\nM.MAX = nil");
    }

    #[test]
    fn type_alias_projects_to_lua_alias() {
        let a = ast::TypeAlias {
            name: "Id".to_string(),
            generics: Vec::new(),
            value: path("number"),
        };
        assert_eq!(project_type_alias(&a, true), "---@alias Id number");
    }
}
