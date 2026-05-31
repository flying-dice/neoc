//! neoc → TypeScript projector. Pure functions mapping a neoc AST type, `FnSig`,
//! `struct`, `enum`, or `const` to TypeScript declaration text.
//!
//! The projection is **total and lossless** (`tasks/ts-interop-inversion.md`):
//! every neoc type has an exact TS form, so a TS consumer of emitted neoc JS
//! calls it type-safely. The composite shapes — `enum`, `Result`, `Option` —
//! mirror the *runtime lowering* the JS backend emits (`lang/DYNAMIC.md` §3.2,
//! §7.1): a value is a tagged object `{ __enum, __tag, … }`, so the `.d.ts`
//! describes the actual emitted JS values and `match` round-trips across the
//! boundary.
//!
//! These functions are wasm-clean (the crate is in the leaf gate): pure string
//! building over `&ast` nodes, no host facilities.

use std::collections::HashSet;
use std::fmt::Write as _;

use neoc_ast as ast;

/// Project a neoc [`ast::Type`] to its TypeScript form.
///
/// The leaf mappings (`tasks/ts-interop-inversion.md` projection table):
/// `number`/`string`/`bool` → `number`/`string`/`boolean`; `()` → `void`;
/// `[T]` → `T[]`; `fn(A) -> B` → `(a0: A) => B`; a generic / user type keeps its
/// name with TS type arguments. `Self` projects to `this` is *not* meaningful in
/// a declaration position, so it maps to the conventional `this`-less form by
/// using the bare type name where one is known; here it falls back to `unknown`
/// only if it escapes a known nominal context (the elaborated AST does not leave
/// a bare `Self` in a projected signature).
#[must_use]
pub fn ts_type(ty: &ast::Type) -> String {
    match ty {
        ast::Type::Unit => "void".to_string(),
        ast::Type::List(inner) => format!("{}[]", ts_type_atom(inner)),
        ast::Type::Fn { params, ret } => {
            let ps = params
                .iter()
                .enumerate()
                .map(|(i, p)| format!("a{i}: {}", ts_type(p)))
                .collect::<Vec<_>>()
                .join(", ");
            let r = ret.as_deref().map_or_else(|| "void".to_string(), ts_type);
            format!("({ps}) => {r}")
        }
        ast::Type::SelfType => "this".to_string(),
        ast::Type::Path(p) => ts_path(p),
    }
}

/// Project a type in a position where a postfix `[]` or a bare member would bind
/// too loosely — a function type or union must be parenthesised before `[]`.
fn ts_type_atom(ty: &ast::Type) -> String {
    match ty {
        ast::Type::Fn { .. } => format!("({})", ts_type(ty)),
        _ => ts_type(ty),
    }
}

/// Project a neoc type path (`number`, `Result<T, E>`, `Foo<Bar>`) to TS.
fn ts_path(p: &ast::TypePath) -> String {
    let Some(seg) = p.segments.last() else {
        return "unknown".to_string();
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
        let args = seg.args.iter().map(ts_type).collect::<Vec<_>>().join(", ");
        format!("{base}<{args}>")
    }
}

/// Render a generic parameter list (`<T, U>`), or empty when there are none.
pub(crate) fn ts_generics(generics: &[ast::GenericParam]) -> String {
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

/// Render a generic parameter list, carrying each bound as a TS `extends` clause
/// (`<E: SqlExecutor>` → `<E extends SqlExecutor>`, `<E: A + B>` →
/// `<E extends A & B>`). A bound is carried only when *every* trait it names is
/// in `in_scope` — a projected/declared type the consumer's `.d.ts` can see;
/// otherwise the whole parameter falls back to bare `<E>` rather than emitting a
/// dangling name. Returns empty when there are no generics.
pub(crate) fn ts_generics_bounded<S: std::hash::BuildHasher>(
    generics: &[ast::GenericParam],
    in_scope: &HashSet<String, S>,
) -> String {
    if generics.is_empty() {
        return String::new();
    }
    let ps = generics
        .iter()
        .map(|g| {
            let names: Vec<&str> = g
                .bounds
                .iter()
                .filter_map(|b| b.segments.last().map(|s| s.name.as_str()))
                .collect();
            if !names.is_empty() && names.iter().all(|n| in_scope.contains(*n)) {
                format!("{} extends {}", g.name, names.join(" & "))
            } else {
                g.name.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("<{ps}>")
}

/// Project a `struct` declaration to a TS `interface`.
///
/// * Named struct `struct S { a: number }` → `export interface S { a: number }`.
/// * Newtype/tuple `struct N(T)` → `export interface N { _0: T }`, matching the
///   runtime lowering that stores the payload at `_0` (`lang/DYNAMIC.md` §3.1).
/// * Unit struct `struct S;` → `export interface S {}`.
///
/// The `__struct` runtime tag is intentionally omitted: a TS consumer constructs
/// neoc structs by calling the emitted functions, never by hand, so the tag is
/// an implementation detail. The fields are the contract.
#[must_use]
pub fn project_struct(s: &ast::StructDecl, exported: bool) -> String {
    let kw = if exported { "export " } else { "" };
    let g = ts_generics(&s.generics);
    let body = match &s.body {
        ast::StructBody::Unit => String::new(),
        ast::StructBody::Tuple(t) => format!(" _0: {}; ", ts_type(t)),
        ast::StructBody::Named(fields) => {
            let mut b = String::from(" ");
            for f in fields {
                let _ = write!(b, "{}: {}; ", f.name, ts_type(&f.ty));
            }
            b
        }
    };
    format!("{kw}interface {}{g} {{{body}}}", s.name)
}

/// Project an `enum` declaration to a discriminated union whose member shapes
/// match the runtime lowering the JS backend emits (`lang/DYNAMIC.md` §3.2):
///
/// * Unit variant `V` → `{ __enum: "E"; __tag: "V" }`
/// * Tuple variant `V(A, B)` → `{ __enum: "E"; __tag: "V"; _0: A; _1: B }`
/// * Struct variant `V { x: T }` → `{ __enum: "E"; __tag: "V"; x: T }`
///
/// The union is `export type E<…> = m1 | m2 | …`. Carrying `__enum` and `__tag`
/// as string-literal members is what lets a `match` (which reads `__tag`)
/// round-trip across the boundary.
#[must_use]
pub fn project_enum(e: &ast::EnumDecl, exported: bool) -> String {
    let kw = if exported { "export " } else { "" };
    let g = ts_generics(&e.generics);
    let members = e
        .variants
        .iter()
        .map(|v| enum_variant_member(&e.name, v))
        .collect::<Vec<_>>()
        .join(" | ");
    let members = if members.is_empty() {
        "never".to_string()
    } else {
        members
    };
    format!("{kw}type {}{g} = {members};", e.name)
}

/// One member object of an enum's discriminated union, mirroring the emitted JS
/// value shape for that variant.
fn enum_variant_member(enum_name: &str, v: &ast::Variant) -> String {
    let head = format!("{{ __enum: \"{enum_name}\"; __tag: \"{}\"", v.name);
    match &v.body {
        ast::VariantBody::Unit => format!("{head} }}"),
        ast::VariantBody::Tuple(tys) => {
            let mut s = head;
            for (i, t) in tys.iter().enumerate() {
                let _ = write!(s, "; _{i}: {}", ts_type(t));
            }
            s.push_str(" }");
            s
        }
        ast::VariantBody::Named(fields) => {
            let mut s = head;
            for f in fields {
                let _ = write!(s, "; {}: {}", f.name, ts_type(&f.ty));
            }
            s.push_str(" }");
            s
        }
    }
}

/// Project a `pub enum`'s constructor namespace to a TS `export declare const`,
/// typing the runtime object the JS backend emits (`emit_pub_enum_namespace`).
/// It merges with the same-named `export type E` union (a TS value+type merge,
/// intended — it reads like an enum), so host code builds variants
/// (`E.Variant(x)`) and reads them (`E.match(v, arms)`) without ever touching the
/// `__tag`/`_0` runtime encoding (`lang/DYNAMIC.md` §3.2, §7.1):
///
/// * unit variant `V` → `V: E`
/// * tuple variant `V(A, B)` → `V(_0: A, _1: B): E`
/// * struct variant `V { x: T }` → `V(fields: { x: T }): E`
/// * the `match` reader → `match<R>(value: E, arms: { V(…): R; … }): R`
#[must_use]
pub fn project_enum_namespace(e: &ast::EnumDecl) -> String {
    let mut s = format!("export declare const {}: {{\n", e.name);
    for v in &e.variants {
        let _ = writeln!(s, "  {};", enum_namespace_member(&e.name, v));
    }
    let arms = e
        .variants
        .iter()
        .map(enum_namespace_arm)
        .collect::<Vec<_>>()
        .join("; ");
    let _ = writeln!(s, "  match<R>(value: {}, arms: {{ {arms} }}): R;", e.name);
    s.push_str("};");
    s
}

/// One constructor member of the namespace const's object type. A unit variant
/// is a property of the enum type; tuple/struct variants are call signatures
/// returning the enum type.
fn enum_namespace_member(enum_name: &str, v: &ast::Variant) -> String {
    match &v.body {
        ast::VariantBody::Unit => format!("{}: {enum_name}", v.name),
        ast::VariantBody::Tuple(tys) => {
            let params = tys
                .iter()
                .enumerate()
                .map(|(i, t)| format!("_{i}: {}", ts_type(t)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{}({params}): {enum_name}", v.name)
        }
        ast::VariantBody::Named(fields) => {
            let obj = fields
                .iter()
                .map(|f| format!("{}: {}", f.name, ts_type(&f.ty)))
                .collect::<Vec<_>>()
                .join("; ");
            format!("{}(fields: {{ {obj} }}): {enum_name}", v.name)
        }
    }
}

/// One arm signature in the `match` reader's `arms` object type. The arm receives
/// the variant's payload (tuple → positional, struct → the field object, unit →
/// none) and returns the shared result `R`.
fn enum_namespace_arm(v: &ast::Variant) -> String {
    match &v.body {
        ast::VariantBody::Unit => format!("{}(): R", v.name),
        ast::VariantBody::Tuple(tys) => {
            let params = tys
                .iter()
                .enumerate()
                .map(|(i, t)| format!("_{i}: {}", ts_type(t)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{}({params}): R", v.name)
        }
        ast::VariantBody::Named(fields) => {
            let obj = fields
                .iter()
                .map(|f| format!("{}: {}", f.name, ts_type(&f.ty)))
                .collect::<Vec<_>>()
                .join("; ");
            format!("{}(fields: {{ {obj} }}): R", v.name)
        }
    }
}

/// Project a function signature to a TS `export declare function` declaration.
///
/// The `self` receiver (a method) is dropped — methods are not free exports;
/// only free `pub fn`s reach this. Each named parameter keeps its name and
/// projected type; the return type projects via [`ts_type`] (absent → `void`).
///
/// `in_scope` is the set of trait/type names the projected `.d.ts` can resolve.
/// A generic bound (`<E: SqlExecutor>`) is carried as a TS `extends` clause when
/// every trait it names is in scope, so the host's injected adapter is checked
/// against the port at the call site; an out-of-scope bound falls back to a bare
/// `<E>` rather than emitting a dangling name.
#[must_use]
pub fn project_fn<S: std::hash::BuildHasher>(
    sig: &ast::FnSig,
    exported: bool,
    in_scope: &HashSet<String, S>,
) -> String {
    let kw = if exported {
        "export declare function "
    } else {
        "declare function "
    };
    let g = ts_generics_bounded(&sig.generics, in_scope);
    let params = sig
        .params
        .iter()
        .filter_map(|p| match p {
            ast::Param::Receiver => None,
            ast::Param::Named { name, ty, .. } => Some(format!("{name}: {}", ts_type(ty))),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let ret = sig.ret.as_ref().map_or_else(|| "void".to_string(), ts_type);
    format!("{kw}{}{g}({params}): {ret};", sig.name)
}

/// Project a `const` declaration to a TS `export declare const`. neoc consts are
/// typed (`const NAME: T = …`), so the declared type projects directly; the
/// value is elided (a `declare` carries no initializer).
#[must_use]
pub fn project_const(c: &ast::ConstDecl, exported: bool) -> String {
    let kw = if exported {
        "export declare const "
    } else {
        "declare const "
    };
    format!("{kw}{}: {};", c.name, ts_type(&c.ty))
}

/// Project a `type` alias to a TS `export type`.
#[must_use]
pub fn project_type_alias(a: &ast::TypeAlias, exported: bool) -> String {
    let kw = if exported { "export " } else { "" };
    let g = ts_generics(&a.generics);
    format!("{kw}type {}{g} = {};", a.name, ts_type(&a.value))
}

/// Project a `trait` declaration to a structural TS `type` alias — the host's
/// port contract. A `type` (not an `interface`) keeps it non-reopenable and
/// consistent with the `enum` union projection.
///
/// * Each method becomes an object member `name(params...): ret`; the `self`
///   receiver is implicit and dropped, exactly as [`project_fn`] drops it.
/// * Param and return types project through [`ts_type`], so a method touching
///   `Result`/`Option` drives the same `neoc-lib` import the other projectors do.
/// * Trait generics carry over (`pub trait Store<T>` → `export type Store<T>`).
/// * Both required and default-bodied methods are listed — the contract is the
///   full method set.
/// * Associated types (`type Item;`) project as object members `Item: unknown`,
///   the least-surprising shape (a member the implementor binds); the methods
///   reference them by name as ordinary types.
#[must_use]
pub fn project_trait(t: &ast::TraitDecl, exported: bool) -> String {
    let kw = if exported { "export " } else { "" };
    let g = ts_generics(&t.generics);
    let mut members: Vec<String> = Vec::new();
    for assoc in &t.assoc_types {
        members.push(format!("  {assoc}: unknown;"));
    }
    for it in &t.items {
        members.push(format!("  {};", trait_method_member(&it.sig)));
    }
    if members.is_empty() {
        return format!("{kw}type {}{g} = {{}};", t.name);
    }
    let body = members.join("\n");
    format!("{kw}type {}{g} = {{\n{body}\n}};", t.name)
}

/// One method member of a trait's projected object type: `name(params...): ret`.
/// The `self` receiver is dropped (implicit on the value); named params keep
/// their name and projected type; the return projects via [`ts_type`].
fn trait_method_member(sig: &ast::FnSig) -> String {
    let params = sig
        .params
        .iter()
        .filter_map(|p| match p {
            ast::Param::Receiver => None,
            ast::Param::Named { name, ty, .. } => Some(format!("{name}: {}", ts_type(ty))),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let ret = sig.ret.as_ref().map_or_else(|| "void".to_string(), ts_type);
    format!("{}({params}): {ret}", sig.name)
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
        assert_eq!(ts_type(&path("number")), "number");
        assert_eq!(ts_type(&path("string")), "string");
        assert_eq!(ts_type(&path("bool")), "boolean");
        assert_eq!(ts_type(&ast::Type::Unit), "void");
    }

    #[test]
    fn list_and_fn_and_generics() {
        assert_eq!(
            ts_type(&ast::Type::List(Box::new(path("number")))),
            "number[]"
        );
        let f = ast::Type::Fn {
            params: vec![path("number")],
            ret: Some(Box::new(path("string"))),
        };
        assert_eq!(ts_type(&f), "(a0: number) => string");
        // A list of functions parenthesises the element type.
        let lf = ast::Type::List(Box::new(ast::Type::Fn {
            params: vec![],
            ret: None,
        }));
        assert_eq!(ts_type(&lf), "(() => void)[]");
        assert_eq!(
            ts_type(&generic("Result", vec![path("number"), path("string")])),
            "Result<number, string>"
        );
    }

    #[test]
    fn struct_named_interface() {
        let s = ast::StructDecl {
            name: "S".to_string(),
            generics: Vec::new(),
            body: ast::StructBody::Named(vec![field("a", path("number"))]),
        };
        assert_eq!(
            project_struct(&s, true),
            "export interface S { a: number; }"
        );
    }

    #[test]
    fn struct_newtype_stores_payload_at_underscore_zero() {
        let s = ast::StructDecl {
            name: "N".to_string(),
            generics: Vec::new(),
            body: ast::StructBody::Tuple(path("number")),
        };
        assert_eq!(
            project_struct(&s, true),
            "export interface N { _0: number; }"
        );
    }

    #[test]
    fn enum_discriminated_union_matches_runtime_tags() {
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
            "export type E = \
             { __enum: \"E\"; __tag: \"A\" } | \
             { __enum: \"E\"; __tag: \"B\"; _0: number } | \
             { __enum: \"E\"; __tag: \"C\"; x: string };"
        );
    }

    #[test]
    fn enum_namespace_const_types_constructors_and_match() {
        // pub enum SqlValue { Int(number), Text(string), Pair { a: number, b: number }, Null }
        let e = ast::EnumDecl {
            name: "SqlValue".to_string(),
            generics: Vec::new(),
            variants: vec![
                ast::Variant {
                    attrs: Vec::new(),
                    name: "Int".to_string(),
                    name_span: ast::Span::default(),
                    body: ast::VariantBody::Tuple(vec![path("number")]),
                },
                ast::Variant {
                    attrs: Vec::new(),
                    name: "Text".to_string(),
                    name_span: ast::Span::default(),
                    body: ast::VariantBody::Tuple(vec![path("string")]),
                },
                ast::Variant {
                    attrs: Vec::new(),
                    name: "Pair".to_string(),
                    name_span: ast::Span::default(),
                    body: ast::VariantBody::Named(vec![
                        field("a", path("number")),
                        field("b", path("number")),
                    ]),
                },
                ast::Variant {
                    attrs: Vec::new(),
                    name: "Null".to_string(),
                    name_span: ast::Span::default(),
                    body: ast::VariantBody::Unit,
                },
            ],
        };
        assert_eq!(
            project_enum_namespace(&e),
            "export declare const SqlValue: {\n  \
             Int(_0: number): SqlValue;\n  \
             Text(_0: string): SqlValue;\n  \
             Pair(fields: { a: number; b: number }): SqlValue;\n  \
             Null: SqlValue;\n  \
             match<R>(value: SqlValue, arms: { Int(_0: number): R; Text(_0: string): R; \
             Pair(fields: { a: number; b: number }): R; Null(): R }): R;\n\
             };"
        );
    }

    #[test]
    fn fn_export_declaration_with_result_return() {
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
            project_fn(&sig, true, &HashSet::new()),
            "export declare function find(id: number): Result<string, string>;"
        );
    }

    #[test]
    fn fn_drops_receiver_and_voids_absent_return() {
        let sig = ast::FnSig {
            name: "tick".to_string(),
            name_span: ast::Span::default(),
            generics: Vec::new(),
            params: vec![ast::Param::Receiver],
            ret: None,
            where_clause: Vec::new(),
        };
        assert_eq!(
            project_fn(&sig, true, &HashSet::new()),
            "export declare function tick(): void;"
        );
    }

    fn receiver_method(
        name: &str,
        params: Vec<(&str, ast::Type)>,
        ret: Option<ast::Type>,
    ) -> ast::TraitItem {
        let mut ps = vec![ast::Param::Receiver];
        for (n, ty) in params {
            ps.push(ast::Param::Named {
                name: n.to_string(),
                ty,
                span: ast::Span::default(),
            });
        }
        ast::TraitItem {
            sig: ast::FnSig {
                name: name.to_string(),
                name_span: ast::Span::default(),
                generics: Vec::new(),
                params: ps,
                ret,
                where_clause: Vec::new(),
            },
            default: None,
        }
    }

    #[test]
    fn trait_projects_to_structural_type_alias() {
        // pub trait SqlExecutor {
        //   fn execute(self, sql: string, params: [SqlValue]) -> Result<number, string>;
        //   fn query(self, sql: string, params: [SqlValue]) -> Result<[[SqlValue]], string>;
        // }
        let sql_value = path("SqlValue");
        let t = ast::TraitDecl {
            name: "SqlExecutor".to_string(),
            generics: Vec::new(),
            supertraits: Vec::new(),
            assoc_types: Vec::new(),
            items: vec![
                receiver_method(
                    "execute",
                    vec![
                        ("sql", path("string")),
                        ("params", ast::Type::List(Box::new(sql_value.clone()))),
                    ],
                    Some(generic("Result", vec![path("number"), path("string")])),
                ),
                receiver_method(
                    "query",
                    vec![
                        ("sql", path("string")),
                        ("params", ast::Type::List(Box::new(sql_value.clone()))),
                    ],
                    Some(generic(
                        "Result",
                        vec![
                            ast::Type::List(Box::new(ast::Type::List(Box::new(sql_value)))),
                            path("string"),
                        ],
                    )),
                ),
            ],
        };
        assert_eq!(
            project_trait(&t, true),
            "export type SqlExecutor = {\n  \
             execute(sql: string, params: SqlValue[]): Result<number, string>;\n  \
             query(sql: string, params: SqlValue[]): Result<SqlValue[][], string>;\n\
             };"
        );
    }

    #[test]
    fn trait_carries_generics_and_assoc_types() {
        // pub trait Store<T> { type Item; fn get(self, k: string) -> T; }
        let t = ast::TraitDecl {
            name: "Store".to_string(),
            generics: vec![ast::GenericParam {
                name: "T".to_string(),
                bounds: Vec::new(),
            }],
            supertraits: Vec::new(),
            assoc_types: vec!["Item".to_string()],
            items: vec![receiver_method(
                "get",
                vec![("k", path("string"))],
                Some(path("T")),
            )],
        };
        assert_eq!(
            project_trait(&t, true),
            "export type Store<T> = {\n  Item: unknown;\n  get(k: string): T;\n};"
        );
    }

    #[test]
    fn fn_bound_carries_as_extends_when_in_scope() {
        // pub fn create<E: SqlExecutor>(db: E) -> number
        let sig = ast::FnSig {
            name: "create".to_string(),
            name_span: ast::Span::default(),
            generics: vec![ast::GenericParam {
                name: "E".to_string(),
                bounds: vec![ast::TypePath {
                    segments: vec![ast::TypeSegment {
                        name: "SqlExecutor".to_string(),
                        args: Vec::new(),
                    }],
                }],
            }],
            params: vec![ast::Param::Named {
                name: "db".to_string(),
                ty: path("E"),
                span: ast::Span::default(),
            }],
            ret: Some(path("number")),
            where_clause: Vec::new(),
        };
        let in_scope: HashSet<String> = ["SqlExecutor".to_string()].into_iter().collect();
        assert_eq!(
            project_fn(&sig, true, &in_scope),
            "export declare function create<E extends SqlExecutor>(db: E): number;"
        );
        // Out of scope → bare <E>, no dangling name.
        assert_eq!(
            project_fn(&sig, true, &HashSet::new()),
            "export declare function create<E>(db: E): number;"
        );
    }

    #[test]
    fn fn_multiple_bounds_become_intersection() {
        let sig = ast::FnSig {
            name: "f".to_string(),
            name_span: ast::Span::default(),
            generics: vec![ast::GenericParam {
                name: "E".to_string(),
                bounds: vec![
                    ast::TypePath {
                        segments: vec![ast::TypeSegment {
                            name: "A".to_string(),
                            args: Vec::new(),
                        }],
                    },
                    ast::TypePath {
                        segments: vec![ast::TypeSegment {
                            name: "B".to_string(),
                            args: Vec::new(),
                        }],
                    },
                ],
            }],
            params: Vec::new(),
            ret: None,
            where_clause: Vec::new(),
        };
        let in_scope: HashSet<String> = ["A".to_string(), "B".to_string()].into_iter().collect();
        assert_eq!(
            project_fn(&sig, true, &in_scope),
            "export declare function f<E extends A & B>(): void;"
        );
    }

    #[test]
    fn const_declaration_elides_value() {
        let c = ast::ConstDecl {
            name: "MAX".to_string(),
            ty: path("number"),
            value: ast::Expr::new(ast::ExprKind::Int("9".to_string()), ast::Span::default()),
        };
        assert_eq!(project_const(&c, true), "export declare const MAX: number;");
    }
}
