//! Elaboration assertions for `neoc_types::elaborate`, driven through the real
//! parser (source → AST → elaborate). Each test checks that a string-typed `+`
//! becomes `BinOp::Concat` while every other `+` stays `BinOp::Add`, and that
//! the rest of the module is left structurally intact.

use neoc_ast::{AssignOp, BinOp, Expr, ExprKind, ItemKind, Module, StmtKind};
use neoc_types::elaborate;

fn parse(src: &str) -> Module {
    let parsed = neoc_parser::parse(src);
    assert!(
        parsed.diagnostics.is_empty(),
        "parser rejected the test program: {:?}",
        parsed.diagnostics
    );
    parsed.module
}

/// The trailing value expression of the first `fn`'s body.
fn first_fn_value(module: &Module) -> &Expr {
    let item = module.items.first().expect("module has at least one item");
    let ItemKind::Fn(f) = &item.kind else {
        panic!("first item is not a fn");
    };
    f.body.value.as_deref().expect("fn body has a value")
}

fn binop(expr: &Expr) -> BinOp {
    match &expr.kind {
        ExprKind::Binary { op, .. } => *op,
        other => panic!("expected a binary expression, found {other:?}"),
    }
}

#[test]
fn string_literals_become_concat() {
    let m = elaborate(&parse(r#"fn f() -> string { "a" + "b" }"#));
    assert_eq!(binop(first_fn_value(&m)), BinOp::Concat);
}

#[test]
fn number_literals_stay_add() {
    let m = elaborate(&parse("fn f() -> number { 1 + 2 }"));
    assert_eq!(binop(first_fn_value(&m)), BinOp::Add);
}

#[test]
fn string_params_become_concat() {
    let m = elaborate(&parse("fn f(a: string, b: string) -> string { a + b }"));
    assert_eq!(binop(first_fn_value(&m)), BinOp::Concat);
}

#[test]
fn number_params_stay_add() {
    let m = elaborate(&parse("fn f(a: number, b: number) -> number { a + b }"));
    assert_eq!(binop(first_fn_value(&m)), BinOp::Add);
}

#[test]
fn string_let_binding_becomes_concat() {
    let src = r#"fn f(name: string) -> string { let prefix = "x: "; prefix + name }"#;
    let m = elaborate(&parse(src));
    assert_eq!(binop(first_fn_value(&m)), BinOp::Concat);
}

#[test]
fn nested_concat_at_the_right_node() {
    // `("x: " + name)` is the argument to `print`; the inner `+` is on strings.
    let src = r#"extern { fn print(s: string); }
fn f(name: string) { print("x: " + name); }"#;
    let m = elaborate(&parse(src));
    let item = m.items.get(1).expect("second item is the fn");
    let ItemKind::Fn(f) = &item.kind else {
        panic!("expected a fn");
    };
    let Some(StmtKind::Expr(Expr {
        kind: ExprKind::Call { args, .. },
        ..
    })) = f.body.stmts.first().map(|s| &s.kind)
    else {
        panic!("expected a call statement");
    };
    let arg = args.first().expect("call has one argument");
    assert_eq!(binop(arg), BinOp::Concat);
}

#[test]
fn unknown_operand_stays_add() {
    // `T` is a generic parameter: its type is ambiguous, so leave `Add`.
    let src = "fn f<T>(a: T, b: T) -> T { a + b }";
    let m = elaborate(&parse(src));
    assert_eq!(binop(first_fn_value(&m)), BinOp::Add);
}

#[test]
fn non_plus_expression_untouched() {
    // A `*` and a non-binary body must survive elaboration unchanged.
    let arith = parse("fn f(a: number, b: number) -> number { a * b }");
    assert_eq!(elaborate(&arith), arith);

    let field = parse("struct P { x: number } fn f(p: P) -> number { p.x }");
    assert_eq!(elaborate(&field), field);
}

#[test]
fn concat_inside_method_body() {
    let src = r"struct Greeter { greeting: string }
impl Greeter {
    fn greet(self, name: string) -> string { self.greeting + name }
}";
    let m = elaborate(&parse(src));
    let item = m.items.get(1).expect("impl block is the second item");
    let ItemKind::Impl(b) = &item.kind else {
        panic!("expected an impl block");
    };
    let method = b.items.first().expect("impl has one method");
    let value = method.body.value.as_deref().expect("method has a value");
    assert_eq!(binop(value), BinOp::Concat);
}

#[test]
fn match_arm_binding_drives_concat() {
    let src = r#"fn f(o: Option<string>) -> string {
    match o {
        Some(s) => s + s,
        None => "",
    }
}"#;
    let m = elaborate(&parse(src));
    let ExprKind::Match { arms, .. } = &first_fn_value(&m).kind else {
        panic!("expected a match");
    };
    assert_eq!(binop(&arms[0].body), BinOp::Concat);
}

#[test]
fn module_otherwise_identical() {
    // Only the string `+` changes; the surrounding structure is preserved.
    let src = r"fn f(a: string, b: number) -> string {
    let n = b + 1;
    a + a
}";
    let parsed = parse(src);
    let m = elaborate(&parsed);
    // The `let n = b + 1` keeps `Add`.
    let item = m.items.first().expect("one item");
    let ItemKind::Fn(f) = &item.kind else {
        panic!("expected fn");
    };
    let Some(StmtKind::Let(l)) = f.body.stmts.first().map(|s| &s.kind) else {
        panic!("expected let");
    };
    assert_eq!(binop(l.value.as_ref().expect("let value")), BinOp::Add);
    // The trailing `a + a` becomes `Concat`.
    assert_eq!(binop(first_fn_value(&m)), BinOp::Concat);
}

/// The first statement of the first `fn`, as an assignment's `(op, value)`.
fn first_assign(module: &Module) -> (AssignOp, &Expr) {
    let item = module.items.first().expect("module has at least one item");
    let ItemKind::Fn(f) = &item.kind else {
        panic!("first item is not a fn");
    };
    let stmt = f.body.stmts.get(1).expect("fn has an assignment statement");
    let StmtKind::Expr(Expr {
        kind: ExprKind::Assign { op, value, .. },
        ..
    }) = &stmt.kind
    else {
        panic!("expected an assignment statement, found {:?}", stmt.kind);
    };
    (*op, value)
}

#[test]
fn string_compound_add_becomes_compound_concat() {
    let m = elaborate(&parse(r#"fn f() { let s = ""; s += "a"; }"#));
    let (op, value) = first_assign(&m);
    assert_eq!(op, AssignOp::Concat);
    assert!(matches!(value.kind, ExprKind::Str(_)), "{value:?}");
}

#[test]
fn number_compound_add_stays_compound() {
    let m = elaborate(&parse("fn f() { let n = 0; n += 1; }"));
    let (op, value) = first_assign(&m);
    assert_eq!(op, AssignOp::Add);
    assert!(matches!(value.kind, ExprKind::Int(_)), "{value:?}");
}

// ---- Option marshalling at the extern boundary (lang/EXTERN.md) ------------

/// The trailing value expression of `fn main`.
fn main_value(m: &Module) -> &Expr {
    let f = m
        .items
        .iter()
        .find_map(|i| match &i.kind {
            ItemKind::Fn(f) if f.sig.name == "main" => Some(f),
            _ => None,
        })
        .expect("fn main");
    f.body.value.as_deref().expect("main has a value")
}

const HOST: &str = "extern { fn put(v: Option<number>) -> Option<number>; }\n";

#[test]
fn extern_call_unwraps_option_arg_and_wraps_option_result() {
    use neoc_ast::MarshalDirection::{FromHost, ToHost};
    let m = elaborate(&parse(&format!(
        "{HOST}fn main() -> Option<number> {{ put(Some(1)) }}"
    )));
    let ExprKind::Marshal { direction, operand } = &main_value(&m).kind else {
        panic!("result not wrapped: {:?}", main_value(&m));
    };
    assert_eq!(*direction, FromHost);
    let ExprKind::Call { args, .. } = &operand.kind else {
        panic!("not a call: {operand:?}");
    };
    assert!(matches!(
        &args[0].kind,
        ExprKind::Marshal {
            direction: ToHost,
            ..
        }
    ));
}

#[test]
fn neoc_fn_call_is_not_marshalled() {
    let m = elaborate(&parse(
        "fn put(v: Option<number>) -> Option<number> { v }\n\
         fn main() -> Option<number> { put(Some(1)) }",
    ));
    let ExprKind::Call { args, .. } = &main_value(&m).kind else {
        panic!("not a plain call: {:?}", main_value(&m));
    };
    assert!(matches!(&args[0].kind, ExprKind::Call { .. }));
}

#[test]
fn local_shadowing_an_extern_name_is_not_marshalled() {
    let m = elaborate(&parse(&format!(
        "{HOST}fn main(put: fn(Option<number>) -> Option<number>) -> Option<number> {{ put(None) }}"
    )));
    assert!(matches!(&main_value(&m).kind, ExprKind::Call { .. }));
}

#[test]
fn extern_fn_value_with_option_becomes_marshalling_closure() {
    let m = elaborate(&parse(&format!(
        "{HOST}fn main() -> fn(Option<number>) -> Option<number> {{ put }}"
    )));
    let ExprKind::Closure { params, body } = &main_value(&m).kind else {
        panic!("not a closure: {:?}", main_value(&m));
    };
    assert_eq!(params.len(), 1);
    assert!(matches!(&body.kind, ExprKind::Marshal { .. }));
}

#[test]
fn extern_fn_value_without_option_is_left_alone() {
    let m = elaborate(&parse(
        "extern { fn get(v: number) -> number; }\nfn main() -> fn(number) -> number { get }",
    ));
    assert!(matches!(&main_value(&m).kind, ExprKind::Path(_)));
}

#[test]
fn extern_struct_option_field_marshals_read_write_and_literal() {
    use neoc_ast::MarshalDirection::{FromHost, ToHost};
    let m = elaborate(&parse(
        "extern { struct S { title: Option<string> } }\n\
         fn main(s: S) -> Option<string> { s.title = Some(\"a\"); let t = S { title: None }; s.title }",
    ));
    let ItemKind::Fn(f) = &m.items[1].kind else {
        panic!("not a fn");
    };
    let StmtKind::Expr(write) = &f.body.stmts[0].kind else {
        panic!("not an expression statement");
    };
    let ExprKind::Assign { target, value, .. } = &write.kind else {
        panic!("not an assignment: {write:?}");
    };
    assert!(matches!(&target.kind, ExprKind::Field { .. }));
    assert!(matches!(
        &value.kind,
        ExprKind::Marshal {
            direction: ToHost,
            ..
        }
    ));
    let StmtKind::Let(l) = &f.body.stmts[1].kind else {
        panic!("not a let");
    };
    let ExprKind::StructLit { fields, .. } = &l.value.as_ref().expect("value").kind else {
        panic!("not a literal");
    };
    assert!(matches!(
        &fields[0].value.as_ref().expect("value").kind,
        ExprKind::Marshal {
            direction: ToHost,
            ..
        }
    ));
    assert!(matches!(
        &main_value(&m).kind,
        ExprKind::Marshal {
            direction: FromHost,
            ..
        }
    ));
}

#[test]
fn extern_fn_value_passed_to_host_stays_raw() {
    let m = elaborate(&parse(
        "extern { fn pcall(f: Unknown, a: Unknown) -> Unknown; fn show(v: Option<number>) -> string; }\n\
         fn main() -> Unknown { pcall(show, 3) }",
    ));
    let ExprKind::Call { args, .. } = &main_value(&m).kind else {
        panic!("not a call: {:?}", main_value(&m));
    };
    assert!(matches!(&args[0].kind, ExprKind::Path(_)), "{:?}", args[0]);
}

#[test]
fn neoc_impl_method_on_extern_type_is_not_marshalled() {
    let m = elaborate(&parse(
        "extern { struct Counter; impl Counter { fn get(self) -> Option<number>; } }\n\
         impl Counter { fn plus(self, extra: Option<number>) -> Option<number> { extra } }\n\
         fn main(c: Counter) -> Option<number> { c.plus(Some(2)) }",
    ));
    let ExprKind::MethodCall { args, .. } = &main_value(&m).kind else {
        panic!("neoc method marshalled: {:?}", main_value(&m));
    };
    assert!(
        matches!(&args[0].kind, ExprKind::Call { .. }),
        "{:?}",
        args[0]
    );
}

#[test]
fn host_method_on_extern_type_beside_neoc_impl_still_marshals() {
    let m = elaborate(&parse(
        "extern { struct Counter; impl Counter { fn get(self) -> Option<number>; } }\n\
         impl Counter { fn twice(self) -> number { 2 } }\n\
         fn main(c: Counter) -> Option<number> { c.get() }",
    ));
    assert!(matches!(&main_value(&m).kind, ExprKind::Marshal { .. }));
}

#[test]
fn aliased_option_param_and_result_marshal() {
    use neoc_ast::MarshalDirection::{FromHost, ToHost};
    let m = elaborate(&parse(
        "type MaybeN = Option<number>;\n\
         extern { fn put(v: MaybeN) -> MaybeN; }\n\
         fn main() -> MaybeN { put(Some(1)) }",
    ));
    let ExprKind::Marshal { direction, operand } = &main_value(&m).kind else {
        panic!("aliased result not wrapped: {:?}", main_value(&m));
    };
    assert_eq!(*direction, FromHost);
    let ExprKind::Call { args, .. } = &operand.kind else {
        panic!("not a call: {operand:?}");
    };
    assert!(matches!(
        &args[0].kind,
        ExprKind::Marshal {
            direction: ToHost,
            ..
        }
    ));
}
