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
fn string_compound_add_becomes_plain_assign_of_concat() {
    let m = elaborate(&parse(r#"fn f() { let s = ""; s += "a"; }"#));
    let (op, value) = first_assign(&m);
    assert_eq!(op, AssignOp::Assign);
    assert_eq!(binop(value), BinOp::Concat);
}

#[test]
fn number_compound_add_stays_compound() {
    let m = elaborate(&parse("fn f() { let n = 0; n += 1; }"));
    let (op, value) = first_assign(&m);
    assert_eq!(op, AssignOp::Add);
    assert!(matches!(value.kind, ExprKind::Int(_)), "{value:?}");
}
