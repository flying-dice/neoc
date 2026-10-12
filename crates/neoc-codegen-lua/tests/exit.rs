//! Leaving a value-position wrapper (`lang/DYNAMIC.md` §4.12): which wrappers
//! take the exit protocol, which keep the plain single-value form, and how the
//! call site dispatches. Behaviour at run time is pinned by the
//! `4-12-exit-*` conformance cases; these assert the emit's shape.
//!
//! Source → parse → elaborate → emit.

use neoc_codegen_lua::emit;

const EXTERN: &str = "extern { fn println(s: string); }\n";

fn lua(body: &str) -> String {
    let src = format!("{EXTERN}{body}");
    let parsed = neoc_parser::parse(&src);
    assert!(parsed.ok(), "parse diagnostics: {:#?}", parsed.diagnostics);
    let module = neoc_types::elaborate(&parsed.module);
    let result = emit(&module);
    assert!(
        result.diagnostics.is_empty(),
        "emit diagnostics: {:#?}",
        result.diagnostics
    );
    result.lua
}

/// The number of wrappers that take the exit protocol.
fn protocol_wrappers(out: &str) -> usize {
    out.matches("local __exit_").count()
}

#[test]
fn plain_wrapper_keeps_the_single_value_form() {
    let out = lua("fn f(c: bool) -> number { let v = if c { 1 } else { 2 }; v }");
    assert_eq!(protocol_wrappers(&out), 0, "{out}");
    assert!(out.contains("local v = (function()"), "{out}");
    assert!(!out.contains("return false,"), "{out}");
}

#[test]
fn return_in_a_block_takes_the_return_exit() {
    let out = lua(r#"fn f(c: bool) -> string {
    let v = { if c { return "early"; } "late" };
    v
}"#);
    assert_eq!(protocol_wrappers(&out), 1, "{out}");
    assert!(out.contains("return 1, \"early\""), "{out}");
    assert!(out.contains("return false, \"late\""), "{out}");
    // The call site returns the value from the function and the wrapper's
    // value otherwise; only the return exit has a branch.
    assert!(
        out.contains("if __exit_1 then\n    return __val_1\n  end"),
        "{out}"
    );
    assert!(out.contains("local v = __val_1"), "{out}");
}

#[test]
fn tail_return_arm_lowers_as_a_statement() {
    let out = lua(
        r"fn f(r: Result<number, string>) -> Result<number, string> {
    let v = match r { Ok(v) => v, Err(e) => return Err(e) };
    Ok(v)
}",
    );
    assert!(out.contains("return 1, Err(e)"), "{out}");
    assert!(out.contains("return false, v"), "{out}");
}

#[test]
fn try_at_function_level_stays_a_one_line_guard() {
    let out =
        lua("fn f(r: Result<number, string>) -> Result<number, string> { let v = r?; Ok(v) }");
    assert_eq!(protocol_wrappers(&out), 0, "{out}");
    assert!(out.contains("== \"None\" then return __try_0 end"), "{out}");
}

#[test]
fn try_in_a_wrapper_takes_the_return_exit() {
    let out = lua(
        r"fn f(r: Result<number, string>) -> Result<number, string> {
    let v = { let x = r?; x + 1 };
    Ok(v)
}",
    );
    assert_eq!(protocol_wrappers(&out), 1, "{out}");
    assert!(out.contains("then return 1, __try_0 end"), "{out}");
}

#[test]
fn exit_passes_through_nested_wrappers() {
    let out = lua(r#"fn f(n: number) -> string {
    let r = if n > 0 { { if n > 5 { return "big"; } "small" } } else { "none" };
    r
}"#);
    assert_eq!(protocol_wrappers(&out), 2, "{out}");
    // The inner call site is inside the outer wrapper: it re-raises the exit.
    assert!(out.contains("return 1, __val_1"), "{out}");
    assert!(out.contains("return 1, \"big\""), "{out}");
}

#[test]
fn break_of_an_outer_loop_takes_the_break_exit() {
    let out = lua(r"fn f() {
    while true {
        let v = { if true { break; } 1 };
    }
}");
    assert_eq!(protocol_wrappers(&out), 1, "{out}");
    assert!(out.contains("return 2\n"), "{out}");
    assert!(
        out.contains("if __exit_1 then\n      break\n    end"),
        "{out}"
    );
}

#[test]
fn continue_of_an_outer_loop_takes_the_continue_exit() {
    let out = lua(r"fn f() {
    for i in 0..3 {
        let v = { if i == 1 { continue; } i };
    }
}");
    assert!(out.contains("return 3\n"), "{out}");
    // The loop is restructured for `continue`; the call site breaks the `repeat`.
    assert!(out.contains("repeat"), "{out}");
}

#[test]
fn break_bound_inside_the_wrapper_keeps_the_plain_form() {
    let out = lua(r"fn f() -> number {
    let v = { let t = 0; while true { t += 1; if t > 1 { break; } } t };
    v
}");
    assert_eq!(protocol_wrappers(&out), 0, "{out}");
    assert!(out.contains("break"), "{out}");
}

#[test]
fn return_in_a_closure_in_a_wrapper_is_the_closures_own() {
    let out = lua(r"fn f() -> number {
    let v = { let g = |y| { if y > 0 { return 1; } 2 }; g(1) };
    v
}");
    assert_eq!(protocol_wrappers(&out), 0, "{out}");
    assert!(out.contains("return 1\n"), "{out}");
}

#[test]
fn closure_try_returns_from_the_closure() {
    let out = lua(
        r"fn f(x: Result<number, string>) -> Result<number, string> {
    let g = |y| { let z = x?; Ok(z + y) };
    g(1)
}",
    );
    assert_eq!(protocol_wrappers(&out), 0, "{out}");
    // The guard sits inside the closure's `function … end`, not before it.
    let guard = out.find("local __try_0").expect(&out);
    let closure = out.find("local g = function(y)").expect(&out);
    assert!(closure < guard, "{out}");
}

#[test]
fn nested_fn_in_a_wrapper_returns_plainly() {
    let out = lua(r"fn f(n: number) -> number {
    let v = { fn g(k: number) -> number { if k > 0 { return 1; } 2 } g(n) };
    v
}");
    assert_eq!(protocol_wrappers(&out), 0, "{out}");
}

#[test]
fn earlier_operands_are_bound_before_a_hoisting_operand() {
    let out = lua(r"fn mark() -> number { 1 }
fn add(a: number, b: number) -> number { a + b }
fn f(c: bool) -> number {
    let n = add(mark(), { if c { return 0; } 1 });
    n
}");
    let spill = out.find("local __op_1 = mark()").expect(&out);
    let call = out.find("local __exit_1").expect(&out);
    assert!(spill < call, "{out}");
    assert!(out.contains("add(__op_1, __val_1)"), "{out}");
}

#[test]
fn right_operand_of_and_runs_under_the_left() {
    let out = lua(r"fn f(a: bool) -> bool {
    let ok = a && { if a { return false; } true };
    ok
}");
    assert!(out.contains("local __op_1 = a\n  if __op_1 then"), "{out}");
    assert!(out.contains("__op_1 = __val_1"), "{out}");
}

#[test]
fn right_operand_of_or_runs_when_the_left_is_false() {
    let out = lua(r"fn f(a: bool) -> bool {
    let ok = a || { if a { return false; } true };
    ok
}");
    assert!(out.contains("if not __op_1 then"), "{out}");
}

#[test]
fn while_condition_that_hoists_is_checked_every_pass() {
    let out = lua(r"fn p(n: number) -> Result<number, string> { Ok(n) }
fn f() -> Result<number, string> {
    let i = 0;
    while p(i)? < 3 { i += 1; }
    Ok(i)
}");
    assert!(out.contains("while true do"), "{out}");
    assert!(out.contains("if not ("), "{out}");
    let guard = out.find("local __try_0").expect(&out);
    let loop_head = out.find("while true do").expect(&out);
    assert!(loop_head < guard, "{out}");
}
