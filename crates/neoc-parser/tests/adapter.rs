//! AST-stage tests: assert the typed AST the adapter produces, independently
//! of any later stage. A bug that only appears in the emitter cannot hide
//! here — these check the AST shape directly.

use neoc_ast as ast;
use neoc_parser::parse;

fn module(src: &str) -> ast::Module {
    let result = parse(src);
    assert!(
        result.ok(),
        "unexpected diagnostics for:\n{src}\n{:#?}",
        result.diagnostics
    );
    result.module
}

/// Wraps an [`ast::ExprKind`] in an [`ast::Expr`] with a default span. Spans
/// compare always-equal, so an expected expression built this way matches the
/// adapter's output regardless of the byte offsets it recorded.
fn e(kind: ast::ExprKind) -> ast::Expr {
    ast::Expr::new(kind, ast::Span::default())
}

#[test]
fn function_with_let_and_return() {
    let m = module("fn add(a: number, b: number) -> number { let s = a + b; return s; }");
    assert_eq!(m.items.len(), 1);
    let ast::ItemKind::Fn(f) = &m.items[0].kind else {
        panic!("expected fn, got {:?}", m.items[0].kind);
    };
    assert_eq!(f.sig.name, "add");
    assert_eq!(f.sig.params.len(), 2);
    assert_eq!(
        f.sig.params[0],
        ast::Param::Named {
            name: "a".into(),
            ty: ast::Type::Path(ast::TypePath {
                segments: vec![ast::TypeSegment {
                    name: "number".into(),
                    args: vec![]
                }],
            }),
            optional: false,
            // Span compares always-equal, so a default is fine here.
            span: ast::Span::default(),
        }
    );
    assert_eq!(f.body.stmts.len(), 2);
    let ast::StmtKind::Let(let_stmt) = &f.body.stmts[0].kind else {
        panic!("expected let");
    };
    assert_eq!(
        let_stmt.pattern,
        ast::Pattern::Binding {
            name: "s".into(),
            span: ast::Span::default()
        }
    );
    assert_eq!(
        let_stmt.value,
        Some(e(ast::ExprKind::Binary {
            op: ast::BinOp::Add,
            lhs: Box::new(e(ast::ExprKind::Path(vec!["a".into()]))),
            rhs: Box::new(e(ast::ExprKind::Path(vec!["b".into()]))),
        }))
    );
}

#[test]
fn struct_enum_and_visibility() {
    let m = module(
        "pub struct Point { x: number, y: number }\n\
         enum Shape { Circle { radius: number }, Move(number, number), Origin }",
    );
    assert_eq!(m.items[0].vis, ast::Visibility::Public);
    let ast::ItemKind::Struct(s) = &m.items[0].kind else {
        panic!("expected struct");
    };
    assert_eq!(s.name, "Point");
    let ast::StructBody::Named(fields) = &s.body else {
        panic!("expected named struct");
    };
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].name, "x");

    let ast::ItemKind::Enum(e) = &m.items[1].kind else {
        panic!("expected enum");
    };
    assert_eq!(e.variants.len(), 3);
    assert!(matches!(e.variants[0].body, ast::VariantBody::Named(_)));
    assert!(matches!(e.variants[1].body, ast::VariantBody::Tuple(_)));
    assert_eq!(e.variants[2].body, ast::VariantBody::Unit);
}

#[test]
fn match_with_guard_and_or_pattern() {
    let m = module(
        "fn classify(n: number) -> string {\n\
           match n { 0 => \"z\", 1 | 2 => \"s\", x if x > 9 => \"big\", _ => \"o\" }\n\
         }",
    );
    let ast::ItemKind::Fn(f) = &m.items[0].kind else {
        panic!("expected fn");
    };
    let Some(ast::Expr {
        kind: ast::ExprKind::Match { arms, .. },
        ..
    }) = f.body.value.as_deref()
    else {
        panic!("expected match as block value");
    };
    assert_eq!(arms.len(), 4);
    assert_eq!(
        arms[0].pattern,
        ast::Pattern::Literal(ast::LiteralPat::Int("0".into()))
    );
    assert!(matches!(arms[1].pattern, ast::Pattern::Or(_)));
    assert_eq!(
        arms[2].pattern,
        ast::Pattern::Binding {
            name: "x".into(),
            span: ast::Span::default()
        }
    );
    assert!(arms[2].guard.is_some());
    assert_eq!(arms[3].pattern, ast::Pattern::Wildcard);
}

#[test]
fn block_bodied_arm_needs_no_separating_comma() {
    // A `=> { … }` arm bounds itself with its `}`, so the comma before the next
    // arm is optional (as in Rust). Without this the `_` arm was swallowed and
    // the match looked non-exhaustive.
    let m = module(
        "fn f(x: number) -> number {\n\
           match x {\n\
             0 => {\n\
               let y = 1;\n\
               y\n\
             }\n\
             _ => 2\n\
           }\n\
         }",
    );
    let ast::ItemKind::Fn(f) = &m.items[0].kind else {
        panic!("expected fn");
    };
    let Some(ast::Expr {
        kind: ast::ExprKind::Match { arms, .. },
        ..
    }) = f.body.value.as_deref()
    else {
        panic!("expected match as block value");
    };
    assert_eq!(arms.len(), 2, "both arms must parse without the comma");
    assert_eq!(
        arms[0].pattern,
        ast::Pattern::Literal(ast::LiteralPat::Int("0".into()))
    );
    assert_eq!(arms[1].pattern, ast::Pattern::Wildcard);
}

#[test]
fn non_block_arm_without_a_comma_is_rejected() {
    // The flip side: a non-block value would run across the arm boundary
    // (`g(0)` then `(1)` reads as `g(0)(1)`), so the comma stays mandatory.
    // Dropping it is a syntax error, not a silent single-arm parse.
    let result = parse(
        "fn g(n: number) -> number { n }\n\
         fn f(x: number) -> number {\n\
           match x {\n\
             0 => g(0)\n\
             1 => 2\n\
             _ => 3\n\
           }\n\
         }",
    );
    assert!(
        !result.ok(),
        "a non-block arm missing its comma must be rejected, not silently parsed"
    );
}

#[test]
fn qualified_unit_variant_pattern_keeps_its_path() {
    // Regression: `Color::Red` arms must carry their segments, not lower to an
    // empty `Pattern::Path([])`, or match exhaustiveness can't see the variant.
    let m = module(
        "fn name(c: Color) -> string {\n\
           match c { Color::Red => \"r\", Color::Green => \"g\", _ => \"?\" }\n\
         }",
    );
    let ast::ItemKind::Fn(f) = &m.items[0].kind else {
        panic!("expected fn");
    };
    let Some(ast::Expr {
        kind: ast::ExprKind::Match { arms, .. },
        ..
    }) = f.body.value.as_deref()
    else {
        panic!("expected match");
    };
    assert_eq!(
        arms[0].pattern,
        ast::Pattern::Path(vec!["Color".into(), "Red".into()])
    );
    assert_eq!(
        arms[1].pattern,
        ast::Pattern::Path(vec!["Color".into(), "Green".into()])
    );
}

#[test]
fn extern_block_walks_fn_struct_impl_mod() {
    let m = module(
        "use math::vec::Vec3;\n\
         #[derive(Clone, Eq)]\n\
         struct P { x: number }\n\
         extern {\n\
             fn now() -> number;\n\
             struct Response;\n\
             struct Changes { changes: number }\n\
             impl Changes { fn total(self) -> number; }\n\
             #[extern(path = \"vnd:rusqlite\")]\n\
             mod rusqlite {\n\
                 fn open(path: string) -> Unknown;\n\
             }\n\
         }",
    );
    let ast::ItemKind::Use(u) = &m.items[0].kind else {
        panic!("expected use");
    };
    assert_eq!(u.path, vec!["math", "vec", "Vec3"]);

    assert_eq!(m.items[1].attrs.len(), 1);
    assert_eq!(m.items[1].attrs[0].items[0].name, "derive");
    assert_eq!(m.items[1].attrs[0].items[0].args.len(), 2);

    let ast::ItemKind::Extern(block) = &m.items[2].kind else {
        panic!("expected extern block");
    };
    // bodyless fn
    assert!(
        matches!(&block.items[0].kind, ast::ExternItemKind::Fn(sig) if sig.name == "now" && sig.ret.is_some())
    );
    // opaque struct
    assert!(matches!(
        &block.items[1].kind,
        ast::ExternItemKind::Struct(s) if s.name == "Response" && matches!(s.body, ast::StructBody::Unit)
    ));
    // struct with fields
    assert!(matches!(
        &block.items[2].kind,
        ast::ExternItemKind::Struct(s) if s.name == "Changes" && matches!(s.body, ast::StructBody::Named(_))
    ));
    // bodyless impl: method body is empty
    let ast::ExternItemKind::Impl(b) = &block.items[3].kind else {
        panic!("expected extern impl");
    };
    assert_eq!(b.items.len(), 1);
    assert_eq!(b.items[0].sig.name, "total");
    assert!(b.items[0].body.stmts.is_empty() && b.items[0].body.value.is_none());
    // mod carrying #[extern(path)]
    let ast::ExternItemKind::Mod(md) = &block.items[4].kind else {
        panic!("expected extern mod");
    };
    assert_eq!(md.name, "rusqlite");
    let attr = ast::ExternAttr::parse(&block.items[4].attrs).expect("extern attr");
    assert_eq!(attr.path.as_deref(), Some("vnd:rusqlite"));
    assert!(matches!(&md.items[0].kind, ast::ExternItemKind::Fn(sig) if sig.name == "open"));
}

#[test]
fn extern_constructor_attr_and_vis_walk() {
    let m = module(
        "extern {\n\
             pub struct Database;\n\
             impl Database {\n\
                 #[extern(constructor)] pub fn new(filename: string);\n\
                 pub fn query(self, sql: string) -> Unknown;\n\
             }\n\
         }",
    );
    let ast::ItemKind::Extern(block) = &m.items[0].kind else {
        panic!("expected extern block");
    };
    assert_eq!(block.items[0].vis, ast::Visibility::Public);
    let ast::ExternItemKind::Impl(b) = &block.items[1].kind else {
        panic!("expected impl");
    };
    let new = &b.items[0];
    assert_eq!(new.sig.name, "new");
    let attr = ast::ExternAttr::parse(&new.attrs).expect("extern attr");
    assert!(attr.constructor);
}

#[test]
fn mod_is_an_identifier_outside_extern() {
    // `mod` is only a keyword inside an extern block; elsewhere an ordinary name.
    let m = module("fn main() { let mod_count = 1; }");
    assert_eq!(m.items.len(), 1);
}

#[test]
fn method_call_and_field_access() {
    let m = module("fn f() { let n = point.translate(1, 2).x; }");
    let ast::ItemKind::Fn(f) = &m.items[0].kind else {
        panic!("expected fn");
    };
    let ast::StmtKind::Let(l) = &f.body.stmts[0].kind else {
        panic!("expected let");
    };
    // `point.translate(1, 2).x` → Field { base: MethodCall { .. }, field: x }
    let ast::ExprKind::Field { base, field } = &l.value.as_ref().unwrap().kind else {
        panic!("expected field access");
    };
    assert_eq!(
        *field,
        ast::FieldAccess::Named("x".into(), ast::Span::default())
    );
    assert!(
        matches!(&base.kind, ast::ExprKind::MethodCall { method, .. } if method == "translate")
    );
}

#[test]
fn expression_span_slices_the_source() {
    // The initialiser's span must point at exactly `foo` in the source so a
    // diagnostic against this expression highlights the identifier, not line 0.
    let src = "fn f() { let x = foo; }";
    let m = module(src);
    let ast::ItemKind::Fn(f) = &m.items[0].kind else {
        panic!("expected fn");
    };
    let ast::StmtKind::Let(l) = &f.body.stmts[0].kind else {
        panic!("expected let");
    };
    let value = l.value.as_ref().expect("let has a value");
    assert_eq!(&src[value.span.start..value.span.end], "foo");
}

#[test]
fn a_nested_syntax_error_is_surfaced() {
    // Regression: the adapter only checked the root's direct children for error
    // nodes, so a malformed construct deeper in the tree — here `pub` on a
    // struct field, which the spec does not allow (§3.4) — parsed silently and
    // let malformed source reach the checker. The whole tree must be scanned.
    let result = parse("struct P {\n  pub x: number,\n}\n");
    assert!(
        !result.ok(),
        "a `pub` field is a syntax error and must produce a diagnostic"
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message == "syntax error"),
        "expected a syntax-error diagnostic, got: {:#?}",
        result.diagnostics
    );
}

#[test]
fn a_let_without_a_terminating_semicolon_is_rejected() {
    // SYNTAX.md §6.1: a `let` is a statement and ends in `;`; only a trailing
    // expression with no `;` is the block value. A missing `;` left a recovery
    // node the adapter used to swallow.
    let result = parse("fn f() -> number {\n  let x = 1\n  x\n}\n");
    assert!(
        !result.ok(),
        "a `let` missing its `;` must produce a diagnostic, got none"
    );
}
