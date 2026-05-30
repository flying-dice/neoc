//! neoc → Luau inline-type projector. Pure functions mapping a neoc AST type,
//! `FnSig`, `struct`, `enum`, or `const` to **native inline Luau type syntax**.
//!
//! This is a *distinct dialect* from the `LuaLS` `---@` comment annotations in
//! [`crate::dlua`]: that projector writes a `.d.lua` sidecar in the
//! `lua-language-server` comment language; this one emits Luau's own first-class
//! inline types (`type Row = { x: number }`, `function f(a: number): R`), so a
//! `.luau` file is **self-typed** with no sidecar. The neoc→type-name mapping is
//! shared in spirit with `dlua` — `bool` → `boolean`, `()` → `nil`, the
//! `Result`/`Option`/enum shapes mirror the runtime tagged tables
//! (`lang/DYNAMIC.md` §3.1, §3.2, §7) — but the surface syntax is Luau's, not the
//! annotation dialect's.
//!
//! These functions are wasm-clean (the crate is in the leaf gate): pure string
//! building over `&ast` nodes, no host facilities.

use std::fmt::Write as _;

use neoc_ast as ast;

/// Project a neoc [`ast::Type`] to its inline Luau type form.
///
/// The leaf mappings (mirroring the runtime values the Lua/Luau backend emits):
/// `number`/`string` keep their names; `bool` → `boolean`; `()` (unit) → `nil`;
/// `[T]` → `{T}` (Luau's array form); `fn(A) -> B` → `(A) -> B`; a generic / user
/// type keeps its name with Luau type arguments (`Result<number, string>`).
/// `Self` has no declaration-position meaning, so it falls back to the
/// conventional `self`.
#[must_use]
pub fn luau_type(ty: &ast::Type) -> String {
    match ty {
        // A neoc unit value lowers to `nil` at runtime.
        ast::Type::Unit => "nil".to_string(),
        // Luau spells a homogeneous array `{T}`.
        ast::Type::List(inner) => format!("{{{}}}", luau_type(inner)),
        ast::Type::Fn { params, ret } => {
            let ps = params.iter().map(luau_type).collect::<Vec<_>>().join(", ");
            let r = ret.as_deref().map_or_else(|| "nil".to_string(), luau_type);
            format!("({ps}) -> {r}")
        }
        ast::Type::SelfType => "self".to_string(),
        ast::Type::Path(p) => luau_path(p),
    }
}

/// Project a neoc type path (`number`, `Result<T, E>`, `Foo<Bar>`) to Luau.
fn luau_path(p: &ast::TypePath) -> String {
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
        let args = seg
            .args
            .iter()
            .map(luau_type)
            .collect::<Vec<_>>()
            .join(", ");
        format!("{base}<{args}>")
    }
}

/// Render a generic parameter list (`<T, U>`), or empty when there are none.
pub(crate) fn luau_generics(generics: &[ast::GenericParam]) -> String {
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

/// Project a `struct` declaration to a Luau `type` alias over a table type,
/// mirroring the emitted tagged table (`lang/DYNAMIC.md` §3.1).
///
/// * Named struct `struct S { a: number }` → `type S = { __struct: "S", a: number }`.
/// * Newtype/tuple `struct N(T)` → `type N = { __struct: "N", [number]: T }`,
///   matching the runtime lowering that stores the payload at integer key `[1]`.
///   A `luau` indexer key must be a type, not a literal (`[1]` is a parse error),
///   so the positional slot is spelled as a `[number]` indexer.
/// * Unit struct `struct S;` → `type S = { __struct: "S" }`.
///
/// The `__struct` runtime tag is carried as a string-literal field — the value's
/// runtime identity (`match`/`Eq`/`Debug` read it), so the `.luau` type describes
/// the actual table. `exported` is accepted for API symmetry with the other
/// projectors; a Luau `type` is module-scoped regardless, so it carries no
/// keyword today.
#[must_use]
pub fn project_struct(s: &ast::StructDecl, exported: bool) -> String {
    let _ = exported;
    let g = luau_generics(&s.generics);
    let body = match &s.body {
        ast::StructBody::Unit => format!("{{ __struct: \"{}\" }}", s.name),
        ast::StructBody::Tuple(t) => {
            format!("{{ __struct: \"{}\", [number]: {} }}", s.name, luau_type(t))
        }
        ast::StructBody::Named(fields) => {
            let mut b = format!("{{ __struct: \"{}\"", s.name);
            for f in fields {
                let _ = write!(b, ", {}: {}", f.name, luau_type(&f.ty));
            }
            b.push_str(" }");
            b
        }
    };
    format!("type {}{g} = {body}", s.name)
}

/// Project an `enum` declaration to a Luau union `type` whose member shapes match
/// the runtime tagged tables the backend emits (`lang/DYNAMIC.md` §3.2):
///
/// * Unit variant `V` → `{ __enum: "E", __tag: "V" }`
/// * Tuple variant `V(A, B)` → `{ __enum: "E", __tag: "V", [number]: A | B }`
/// * Struct variant `V { x: T }` → `{ __enum: "E", __tag: "V", x: T }`
///
/// The union is `type E<…> = m1 | m2 | …`. Carrying `__enum`/`__tag` as
/// string-literal fields is what lets a `match` (which reads `__tag`) narrow and
/// round-trip across the boundary.
#[must_use]
pub fn project_enum(e: &ast::EnumDecl, exported: bool) -> String {
    let _ = exported;
    let g = luau_generics(&e.generics);
    let members = e
        .variants
        .iter()
        .map(|v| enum_variant_member(&e.name, v))
        .collect::<Vec<_>>()
        .join(" | ");
    // An empty enum has no inhabitant; Luau spells the bottom as `nil`.
    let members = if members.is_empty() {
        "nil".to_string()
    } else {
        members
    };
    format!("type {}{g} = {members}", e.name)
}

/// One member table type of an enum's union, mirroring the emitted Luau value
/// shape for that variant. The discriminant fields (`__enum`, `__tag`) are
/// string literals so a consumer narrows on them.
fn enum_variant_member(enum_name: &str, v: &ast::Variant) -> String {
    let head = format!("{{ __enum: \"{enum_name}\", __tag: \"{}\"", v.name);
    match &v.body {
        ast::VariantBody::Unit => format!("{head} }}"),
        ast::VariantBody::Tuple(tys) => {
            // Tuple payload lives at integer keys `[1]`, `[2]`, … (§3.2). A `luau`
            // table-type indexer key must be a type — `[1]` is a parse error — so
            // the positional slots collapse to one `[number]` indexer whose value
            // type is the union of the positional field types (deduplicated).
            let mut union: Vec<String> = Vec::new();
            for t in tys {
                let ty = luau_type(t);
                if !union.contains(&ty) {
                    union.push(ty);
                }
            }
            format!("{head}, [number]: {} }}", union.join(" | "))
        }
        ast::VariantBody::Named(fields) => {
            let mut s = head;
            for f in fields {
                let _ = write!(s, ", {}: {}", f.name, luau_type(&f.ty));
            }
            s.push_str(" }");
            s
        }
    }
}

/// Project a `type` alias to a Luau `type`.
#[must_use]
pub fn project_type_alias(a: &ast::TypeAlias, exported: bool) -> String {
    let _ = exported;
    let g = luau_generics(&a.generics);
    format!("type {}{g} = {}", a.name, luau_type(&a.value))
}

/// The inline-Luau parameter list and return-type suffix for a function header.
///
/// Returns `(params, ret)` where `params` is `name: T, …` (a receiver becomes the
/// untyped `self`) and `ret` is `: T` (empty when the signature has no return —
/// Luau then infers it). Used by the emitter to weave typed headers under
/// [`crate::HeaderStyle::Luau`] without re-deriving the parameter names the
/// untyped path computes.
#[must_use]
pub(crate) fn typed_params_and_ret(sig: &ast::FnSig) -> (String, String) {
    let params = sig
        .params
        .iter()
        .map(|p| match p {
            ast::Param::Receiver => "self".to_string(),
            ast::Param::Named { name, ty, .. } => format!("{name}: {}", luau_type(ty)),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let ret = sig
        .ret
        .as_ref()
        .map_or_else(String::new, |t| format!(": {}", luau_type(t)));
    (params, ret)
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
            optional: false,
        }
    }

    #[test]
    fn primitives_and_unit() {
        assert_eq!(luau_type(&path("number")), "number");
        assert_eq!(luau_type(&path("string")), "string");
        assert_eq!(luau_type(&path("bool")), "boolean");
        assert_eq!(luau_type(&ast::Type::Unit), "nil");
    }

    #[test]
    fn list_is_brace_array_and_fn_is_arrow() {
        assert_eq!(
            luau_type(&ast::Type::List(Box::new(path("number")))),
            "{number}"
        );
        let f = ast::Type::Fn {
            params: vec![path("number")],
            ret: Some(Box::new(path("string"))),
        };
        assert_eq!(luau_type(&f), "(number) -> string");
        let nf = ast::Type::Fn {
            params: vec![],
            ret: None,
        };
        assert_eq!(luau_type(&nf), "() -> nil");
        assert_eq!(
            luau_type(&generic("Result", vec![path("number"), path("string")])),
            "Result<number, string>"
        );
    }

    #[test]
    fn struct_named_type_alias_with_tag() {
        let s = ast::StructDecl {
            name: "Row".to_string(),
            generics: Vec::new(),
            body: ast::StructBody::Named(vec![field("x", path("number"))]),
        };
        assert_eq!(
            project_struct(&s, true),
            "type Row = { __struct: \"Row\", x: number }"
        );
    }

    #[test]
    fn struct_newtype_payload_at_number_indexer() {
        // A `luau` indexer key must be a type, not a literal — the positional
        // slot at runtime key `[1]` is projected as a `[number]` indexer.
        let s = ast::StructDecl {
            name: "N".to_string(),
            generics: Vec::new(),
            body: ast::StructBody::Tuple(path("number")),
        };
        assert_eq!(
            project_struct(&s, true),
            "type N = { __struct: \"N\", [number]: number }"
        );
    }

    #[test]
    fn struct_unit_type_alias_tag_only() {
        let s = ast::StructDecl {
            name: "U".to_string(),
            generics: Vec::new(),
            body: ast::StructBody::Unit,
        };
        assert_eq!(project_struct(&s, true), "type U = { __struct: \"U\" }");
    }

    #[test]
    fn enum_union_matches_runtime_tags() {
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
            "type E = \
             { __enum: \"E\", __tag: \"A\" } | \
             { __enum: \"E\", __tag: \"B\", [number]: number } | \
             { __enum: \"E\", __tag: \"C\", x: string }"
        );
    }

    #[test]
    fn enum_two_field_tuple_variant_unions_positional_types() {
        // enum P { Pair(number, string) } — two positional slots collapse to one
        // `[number]` indexer over the union of their types.
        let e = ast::EnumDecl {
            name: "P".to_string(),
            generics: Vec::new(),
            variants: vec![ast::Variant {
                attrs: Vec::new(),
                name: "Pair".to_string(),
                name_span: ast::Span::default(),
                body: ast::VariantBody::Tuple(vec![path("number"), path("string")]),
            }],
        };
        assert_eq!(
            project_enum(&e, true),
            "type P = { __enum: \"P\", __tag: \"Pair\", [number]: number | string }"
        );
    }

    #[test]
    fn enum_two_field_tuple_variant_dedups_repeated_types() {
        // Repeated positional types deduplicate to a single union member.
        let e = ast::EnumDecl {
            name: "P".to_string(),
            generics: Vec::new(),
            variants: vec![ast::Variant {
                attrs: Vec::new(),
                name: "Pair".to_string(),
                name_span: ast::Span::default(),
                body: ast::VariantBody::Tuple(vec![path("number"), path("number")]),
            }],
        };
        assert_eq!(
            project_enum(&e, true),
            "type P = { __enum: \"P\", __tag: \"Pair\", [number]: number }"
        );
    }

    #[test]
    fn type_alias_projects_to_luau_type() {
        let a = ast::TypeAlias {
            name: "Id".to_string(),
            generics: Vec::new(),
            value: path("number"),
        };
        assert_eq!(project_type_alias(&a, true), "type Id = number");
    }

    #[test]
    fn typed_header_carries_params_and_return() {
        // fn find(id: number) -> Result<string, string>
        let sig = ast::FnSig {
            name: "find".to_string(),
            name_span: ast::Span::default(),
            generics: Vec::new(),
            params: vec![ast::Param::Named {
                name: "id".to_string(),
                ty: path("number"),
                optional: false,
                span: ast::Span::default(),
            }],
            ret: Some(generic("Result", vec![path("string"), path("string")])),
            where_clause: Vec::new(),
        };
        let (params, ret) = typed_params_and_ret(&sig);
        assert_eq!(params, "id: number");
        assert_eq!(ret, ": Result<string, string>");
    }

    #[test]
    fn typed_header_receiver_is_untyped_self_and_absent_return_is_empty() {
        let sig = ast::FnSig {
            name: "tick".to_string(),
            name_span: ast::Span::default(),
            generics: Vec::new(),
            params: vec![ast::Param::Receiver],
            ret: None,
            where_clause: Vec::new(),
        };
        let (params, ret) = typed_params_and_ret(&sig);
        assert_eq!(params, "self");
        assert_eq!(ret, "");
    }
}
