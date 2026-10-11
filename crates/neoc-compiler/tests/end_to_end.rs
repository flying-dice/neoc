//! End-to-end tests: transpile neoc → Lua, run the Lua, assert the output.
//!
//! This is the final stage. The per-stage tests (CST in neoc-grammar, AST in
//! neoc-parser, emit in neoc-codegen-lua) already pin each boundary, so a
//! failure here points at integration, not at a single stage hiding a bug. The
//! emitted Lua runs in-process on `LuaJIT` (`tests/common`).

mod common;

/// Transpile, append a call to the entry function, run, and return stdout.
fn transpile_and_run(source: &str, entry: &str) -> String {
    let out = neoc_compiler::transpile(source, neoc_compiler::Target::Lua);
    assert!(out.ok(), "unexpected diagnostics: {:#?}", out.diagnostics);
    let program = format!("{}\n{entry}()\n", out.output);
    common::run_lua(&program)
        .unwrap_or_else(|e| panic!("lua failed:\n{e}\n--- source ---\n{program}"))
}

/// The harness must give emitted Lua the runtime's environment, not a full stdlib:
/// a lowering that leans on a global `neoc run` strips has to fail here too.
#[test]
fn harness_runs_in_the_runtime_sandbox() {
    let probe = "print(load == nil, loadstring == nil, setfenv == nil, getfenv == nil, \
                 collectgarbage == nil, string.dump == nil, os == nil, io == nil, \
                 package == nil, jit == nil, debug == nil)\n";
    let stdout = common::run_lua(probe).expect("probe runs");
    assert_eq!(stdout.trim(), ["true"; 11].join("\t"));
}

#[test]
fn fibonacci_runs() {
    let src = "\
extern { fn print(n: number); }
fn fib(n: number) -> number {
  if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
}
fn main() {
  print(fib(10));
}";
    let stdout = transpile_and_run(src, "main");
    assert_eq!(stdout.trim(), "55");
}

#[test]
fn loops_and_accumulation_run() {
    let src = "\
extern { fn print(n: number); }
fn sum_to(n: number) -> number {
  let total = 0;
  for i in 1..n { total = total + i; }
  total
}
fn main() {
  print(sum_to(11));
}";
    let stdout = transpile_and_run(src, "main");
    // 1 + 2 + ... + 10 = 55 (1..11 is exclusive of 11).
    assert_eq!(stdout.trim(), "55");
}

#[test]
fn string_concat_runs() {
    // `+` on strings must lower to Lua `..`, not `+`. The elaboration pass
    // rewrites the string-typed `Add` to `Concat`; the emitter picks `..`.
    let src = "\
extern { fn print(s: string); }
fn greet(name: string) -> string {
  \"hello, \" + name
}
fn main() {
  print(greet(\"world\"));
}";
    let stdout = transpile_and_run(src, "main");
    assert_eq!(stdout.trim(), "hello, world");
}

#[test]
fn match_over_enum_runs() {
    let src = "\
extern { fn print(s: string); }
enum Shape { Circle, Square, Triangle }
fn name(s: Shape) -> string {
  match s { Shape::Circle => \"circle\", Shape::Square => \"square\", _ => \"other\" }
}
fn main() {
  print(name(Shape::Square));
}";
    let stdout = transpile_and_run(src, "main");
    assert_eq!(stdout.trim(), "square");
}

#[test]
fn continue_and_break_run() {
    // `continue` skips i == 3; `break` exits at i == 7. The sum is
    // 0+1+2+4+5+6 = 18. Exercises the repeat/break restructuring under Lua 5.1.
    let src = "\
extern { fn print(n: number); }
fn main() {
  let acc = 0;
  for i in 0..10 {
    if i == 3 { continue }
    if i == 7 { break }
    acc = acc + i;
  }
  print(acc);
}";
    let stdout = transpile_and_run(src, "main");
    assert_eq!(stdout.trim(), "18");
}

#[test]
fn let_struct_destructure_runs() {
    let src = "\
extern { fn print(n: number); }
struct Point { x: number, y: number }
fn main() {
  let p = Point { x: 3, y: 4 };
  let Point { x, y } = p;
  print(x + y);
}";
    let stdout = transpile_and_run(src, "main");
    assert_eq!(stdout.trim(), "7");
}

#[test]
fn functional_struct_update_runs() {
    // `..p` copies p's fields; the explicit `x: 9` overrides. y stays 2.
    let src = "\
extern { fn print(n: number); }
struct Point { x: number, y: number }
fn main() {
  let p = Point { x: 1, y: 2 };
  let q = Point { x: 9, ..p };
  print(q.x);
  print(q.y);
}";
    let stdout = transpile_and_run(src, "main");
    assert_eq!(stdout.trim(), "9\n2");
}

#[test]
fn enum_method_dispatch_runs() {
    // An `impl` method on an enum dispatches via the variant's metatable.
    let src = "\
extern { fn print(s: string); }
enum Dir { N, S, E, W }
impl Dir {
  fn name(self) -> string {
    match self { Dir::N => \"north\", Dir::S => \"south\", Dir::E => \"east\", Dir::W => \"west\" }
  }
}
fn main() {
  let d = Dir::E;
  print(d.name());
}";
    let stdout = transpile_and_run(src, "main");
    assert_eq!(stdout.trim(), "east");
}

#[test]
fn newtype_construction_and_field_access_run() {
    // `Id(42)` constructs via `.new` (carries the tag/metatable); `.0` reads the
    // positional payload (`[1]`).
    let src = "\
extern { fn print(n: number); }
struct Id(number);
fn main() {
  let id = Id(42);
  print(id.0);
}";
    let stdout = transpile_and_run(src, "main");
    assert_eq!(stdout.trim(), "42");
}

#[test]
fn derived_methods_callable_from_neoc() {
    // Derived methods are now known to the type checker, so neoc calls them
    // directly. The clone is a full value (carries the metatable), so methods
    // dispatch on it too — `q.eq(p)` exercises that.
    let src = "\
extern { fn print(b: bool); }
#[derive(Clone, Eq)]
struct Point { x: number, y: number }
fn main() {
  let p = Point { x: 1, y: 2 };
  let q = p.clone();
  print(p.eq(q));
  print(q.eq(p));
}";
    let stdout = transpile_and_run(src, "main");
    assert_eq!(stdout.trim(), "true\ntrue");
}

/// Run `source` with `host` (Lua defining the `ext fn`s) loaded first, then
/// call `entry`.
fn run_hosted(host: &str, source: &str, entry: &str) -> Result<String, String> {
    let out = neoc_compiler::transpile(source, neoc_compiler::Target::Lua);
    assert!(out.ok(), "unexpected diagnostics: {:#?}", out.diagnostics);
    common::run_lua(&format!("{host}\n{}\n{entry}()\n", out.output))
}

/// `source` with the loop-carried append rewrite (`lang/DYNAMIC.md` §4.11)
/// disabled: a closure that captures `s` blocks it (condition 4).
fn without_concat_rewrite(source: &str) -> String {
    let plain = source.replace(r#"let s = "";"#, r#"let s = ""; let peek = || s;"#);
    assert_ne!(
        plain, source,
        "no `let s = \"\";` to disable the rewrite at"
    );
    plain
}

/// Run `source` and its rewrite-free twin; §4.11 requires the same result.
/// Errors compare by message: the twin's extra line shifts every location.
fn assert_same_as_plain_concat(host: &str, source: &str) -> Result<String, String> {
    let run = |src: &str| run_hosted(host, src, "main").map_err(|e| error_message(&e));
    let rewritten = run(source);
    let plain = run(&without_concat_rewrite(source));
    assert_eq!(rewritten, plain, "--- source ---\n{source}");
    rewritten
}

/// The first line of a Lua error with its `chunk:line:` locations removed.
fn error_message(err: &str) -> String {
    let first = err.lines().next().unwrap_or_default();
    first
        .split(' ')
        .filter(|word| !word.starts_with("conformance:"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn concat_rewrite_matches_plain_when_an_iife_returns() {
    // A loop inside a valued block / `if` emits inside an IIFE, where `return`
    // and `?` leave only the IIFE (#110). Whatever that does, the rewrite must
    // do the same. (`h(1)` is left out: under #110 its `Err` lands in `tag`
    // and the `+` raises, rewrite or not.)
    let src = r#"
extern { fn print(s: string); }
fn step(i: number, limit: number) -> Result<number, string> {
    if i == limit { Err("stop") } else { Ok(i) }
}
fn f(limit: number) -> string {
    let s = "";
    let tag = { for i in 0..5 { s += "x"; if i == limit { return "early"; } } "done" };
    s + "|" + tag
}
fn g(limit: number) -> string {
    let s = "";
    let tag = if limit >= 0 { for i in 0..5 { s += "y"; if i == limit { return s; } } "done" } else { "neg" };
    s + "|" + tag
}
fn h(limit: number) -> Result<string, string> {
    let s = "";
    let tag = { for i in 0..5 { s += "z"; let n = step(i, limit)?; } "done" };
    Ok(s + "|" + tag)
}
fn show(r: Result<string, string>) -> string {
    match r { Ok(v) => v, Err(e) => e }
}
fn main() {
    print(f(1)); print(f(99));
    print(g(1)); print(g(99));
    print(show(h(99)));
}"#;
    let out = assert_same_as_plain_concat("", src).expect("runs");
    assert!(out.starts_with("xx|early\n"), "{out}");
}

#[test]
fn concat_rewrite_raises_like_plain_on_a_nil_from_the_host() {
    // The host breaks `part`'s `string` contract; plain `..` raises at the
    // append, and so must the accumulator push.
    let host = r#"part = function(i) if i == 1 then return nil end return "p" .. i end"#;
    let src = r#"
extern { fn print(s: string); fn part(i: number) -> string; }
fn main() {
    let s = "";
    for i in 0..4 { s += part(i); }
    print(s);
}"#;
    let err = assert_same_as_plain_concat(host, src).expect_err("raises");
    assert!(err.contains("attempt to concatenate"), "{err}");
}

#[test]
fn compound_assign_evaluates_the_place_once() {
    // `STATIC.md` §4.1: `?` in the index runs `tick` once, and the read and the
    // write hit the same element.
    let host = r#"calls = 0
tick = function() calls = calls + 1 return { __enum = "Result", __tag = "Ok", calls - 1 } end
count = function() return calls end"#;
    let src = r#"
extern {
    fn print(s: string);
    fn printn(n: number);
    fn tick() -> Result<number, string>;
    fn count() -> number;
}
fn run() -> Result<string, string> {
    let xs = ["a", "b"];
    xs[tick()?] += "!";
    Ok(xs[0] + xs[1])
}
fn main() {
    match run() { Ok(v) => print(v), Err(e) => print(e) }
    printn(count());
}"#;
    let out = run_hosted(&format!("printn = print\n{host}"), src, "main").expect("runs");
    assert_eq!(out, "a!b\n1\n");
}
