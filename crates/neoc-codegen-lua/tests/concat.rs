//! Loop-carried string append lowering (`lang/DYNAMIC.md` §4.11): when the
//! emitter accumulates appends in a table and folds with `table.concat`, and —
//! as important — every shape where it must keep the plain `..`.
//!
//! Source → parse → elaborate → emit, since the append is recognised by the
//! `Concat` operator the elaboration pass marks.

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

/// Asserts the emit kept the plain `..` append: no fold, and the append is
/// there (so the case is not vacuously "no fold" because nothing was emitted).
fn assert_plain(out: &str) {
    assert_eq!(folds(out), 0, "{out}");
    assert!(out.contains(".. s") || out.contains("s .. "), "{out}");
}

/// Whether the emit accumulated (one `table.concat` fold per accumulator).
fn folds(out: &str) -> usize {
    out.matches("table.concat(").count()
}

#[test]
fn for_loop_append_accumulates() {
    let out = lua(r#"fn main() {
    let s = "a";
    for i in 0..3 { s = s + "x"; }
    println(s);
}"#);
    assert!(out.contains("local __sb_1 = { s }"), "{out}");
    assert!(out.contains("__sb_1[#__sb_1 + 1] = \"x\""), "{out}");
    // The fold follows the loop's `end` and precedes the read.
    let end = out
        .find("    end\n    s = table.concat(__sb_1)")
        .expect(&out);
    assert!(end < out.find("println(s)").expect(&out), "{out}");
    assert!(!out.contains(".. \"x\""), "{out}");
}

#[test]
fn compound_assign_accumulates() {
    let out = lua(r#"fn main() {
    let s = "";
    while true { s += "x"; break }
    println(s);
}"#);
    assert!(out.contains("__sb_1[#__sb_1 + 1] = \"x\""), "{out}");
}

#[test]
fn append_chain_pushes_each_part_in_order() {
    let out = lua(r#"fn main() {
    let s = "";
    for i in 0..3 { s = s + "a" + "b" + "c"; }
    println(s);
}"#);
    let a = out.find("= \"a\"").expect(&out);
    let b = out.find("= \"b\"").expect(&out);
    let c = out.find("= \"c\"").expect(&out);
    assert!(a < b && b < c, "{out}");
}

#[test]
fn break_still_folds_after_the_loop() {
    let out = lua(r#"fn main() {
    let s = "";
    loop { s += "x"; if s == "xxx" { break } }
    println(s);
}"#);
    // The `break` condition reads the string: not accumulated. Control: a
    // break that does not read it is.
    assert_eq!(folds(&out), 0, "{out}");
    let out = lua(r#"fn main() {
    let s = "";
    let n = 0;
    loop { n = n + 1; s += "x"; if n == 3 { break } }
    println(s);
}"#);
    assert_eq!(folds(&out), 1, "{out}");
    let brk = out.find("break").expect(&out);
    assert!(brk < out.find("table.concat").expect(&out), "{out}");
}

#[test]
fn return_of_the_string_folds_in_place() {
    let out = lua(r#"fn f(n: number) -> string {
    let s = "";
    for i in 0..9 { s += "x"; if i == n { return s; } }
    s
}"#);
    assert!(out.contains("return table.concat(__sb_1)"), "{out}");
    assert!(out.contains("s = table.concat(__sb_1)"), "{out}");
}

#[test]
fn return_of_another_value_leaves_the_accumulator_alone() {
    let out = lua(r#"fn f(n: number) -> number {
    let s = "";
    for i in 0..9 { s += "x"; if i == n { return i; } }
    0
}"#);
    assert_eq!(folds(&out), 1, "{out}");
    assert!(out.contains("return i"), "{out}");
}

#[test]
fn nested_loops_share_the_outermost_accumulator() {
    let out = lua(r#"fn main() {
    let s = "";
    for i in 0..2 { for j in 0..2 { s += "x"; } s += "/"; }
    println(s);
}"#);
    assert_eq!(out.matches("local __sb_").count(), 1, "{out}");
    assert_eq!(folds(&out), 1, "{out}");
}

#[test]
fn inner_loop_accumulates_when_the_outer_reads_the_string() {
    let out = lua(r#"fn main() {
    let s = "";
    for i in 0..2 { for j in 0..2 { s += "x"; } println(s); }
}"#);
    assert_eq!(out.matches("local __sb_").count(), 1, "{out}");
    // The fold sits inside the outer loop, ahead of the read.
    assert!(out.find("table.concat").expect(&out) < out.find("println(s)").expect(&out));
}

#[test]
fn two_strings_get_two_accumulators() {
    let out = lua(r#"fn main() {
    let a = "";
    let b = "";
    for i in 0..2 { a += "x"; b += "y"; }
    println(a + b);
}"#);
    assert!(out.contains("local __sb_1 = { a }"), "{out}");
    assert!(out.contains("local __sb_2 = { b }"), "{out}");
}

#[test]
fn continue_loops_accumulate() {
    let out = lua(r#"fn main() {
    let s = "";
    for i in 0..4 { if i == 1 { continue } s += "x"; }
    println(s);
}"#);
    assert_eq!(folds(&out), 1, "{out}");
}

// ---- shapes that must keep plain `..` --------------------------------------

#[test]
fn read_in_body_is_not_rewritten() {
    let out = lua(r#"fn main() {
    let s = "";
    for i in 0..3 { s += "x"; println(s); }
}"#);
    assert_eq!(folds(&out), 0, "{out}");
    assert!(out.contains("s = (s .. \"x\")"), "{out}");
}

#[test]
fn read_in_condition_is_not_rewritten() {
    let out = lua(r#"fn main() {
    let s = "";
    while s != "xxx" { s += "x"; }
}"#);
    assert_plain(&out);
}

#[test]
fn read_in_the_appended_value_is_not_rewritten() {
    let out = lua(r#"fn main() {
    let s = "";
    for i in 0..3 { s = s + s; }
}"#);
    assert_plain(&out);
}

#[test]
fn prepend_is_not_rewritten() {
    let out = lua(r#"fn main() {
    let s = "";
    for i in 0..3 { s = "x" + s; }
}"#);
    assert_plain(&out);
}

#[test]
fn reassignment_is_not_rewritten() {
    let out = lua(r#"fn main() {
    let s = "";
    for i in 0..3 { s += "x"; s = "y"; }
}"#);
    assert_plain(&out);
}

#[test]
fn closure_defined_before_the_loop_blocks_the_rewrite() {
    let out = lua(r#"fn main() {
    let s = "";
    let peek = || s;
    for i in 0..3 { s += "x"; }
    println(peek());
}"#);
    assert_plain(&out);
}

#[test]
fn closure_defined_after_the_loop_blocks_the_rewrite() {
    let out = lua(r#"fn main() {
    let s = "";
    for i in 0..3 { s += "x"; }
    let peek = || s;
    println(peek());
}"#);
    assert_plain(&out);
}

#[test]
fn closure_inside_the_loop_blocks_the_rewrite() {
    let out = lua(r#"fn main() {
    let s = "";
    for i in 0..3 { s += "x"; let f = || s; }
}"#);
    assert_plain(&out);
}

#[test]
fn loop_inside_a_closure_is_not_rewritten() {
    let out = lua(r#"fn main() {
    let s = "";
    let f = || { for i in 0..3 { s += "x"; } };
    f();
    println(s);
}"#);
    assert_plain(&out);
}

#[test]
fn nested_function_mentioning_the_string_blocks_the_rewrite() {
    let out = lua(r#"fn main() {
    let s = "";
    fn peek() -> string { s }
    for i in 0..3 { s += "x"; }
    println(peek());
}"#);
    assert_plain(&out);
}

#[test]
fn rebinding_inside_the_loop_is_not_rewritten() {
    let out = lua(r#"fn main() {
    let s = "";
    for i in 0..3 { let s = "inner"; s += "x"; }
}"#);
    assert_plain(&out);
}

#[test]
fn struct_shorthand_counts_as_a_read() {
    let out = lua(r#"struct Box { s: string }
fn main() {
    let s = "";
    for i in 0..3 { s += "x"; let b = Box { s }; }
}"#);
    assert_plain(&out);
}

#[test]
fn parameters_are_not_rewritten() {
    let out = lua(r#"fn f(s: string) -> string {
    for i in 0..3 { s += "x"; }
    s
}"#);
    assert_plain(&out);
}

#[test]
fn numeric_compound_assign_is_untouched() {
    let out = lua("fn main() {
    let n = 0;
    for i in 0..3 { n += 1; }
}");
    assert_eq!(folds(&out), 0, "{out}");
    assert!(out.contains("n = n + 1"), "{out}");
}

#[test]
fn a_later_shadow_does_not_block_the_first_binding() {
    let out = lua(r#"fn main() {
    let s = "";
    for i in 0..3 { s += "x"; }
    println(s);
    let s = "again";
    let peek = || s;
    println(peek());
}"#);
    assert_eq!(folds(&out), 1, "{out}");
}
