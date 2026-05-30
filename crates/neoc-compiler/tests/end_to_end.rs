//! End-to-end tests: transpile neoc → Luau, run the Luau, assert the output.
//!
//! This is the final stage. The per-stage tests (CST in neoc-grammar, AST in
//! neoc-parser, emit in neoc-codegen-luau) already pin each boundary, so a
//! failure here points at integration, not at a single stage hiding a bug. Luau
//! is the typed superset neoc emits; it runs on the `luau` interpreter (which
//! accepts the inline type annotations plain Lua would reject).
//!
//! Skipped automatically when no `luau` interpreter is on PATH.

use std::io::Write;
use std::process::Command;

/// Run Luau source, returning its stdout. `None` if no interpreter is present.
fn run_luau(source: &str) -> Option<String> {
    if Command::new("luau").arg("--version").output().is_err() {
        return None;
    }

    let mut file = tempfile(source);
    let path = file.0.clone();
    file.1.flush().expect("flush temp file");
    let output = Command::new("luau").arg(&path).output().expect("run luau");
    assert!(
        output.status.success(),
        "luau failed:\n{}\n--- source ---\n{source}",
        String::from_utf8_lossy(&output.stderr)
    );
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Write `contents` to a uniquely named temp file; return its path and handle.
/// The name combines the pid with a process-wide atomic counter so parallel
/// tests never collide (a timestamp alone can, under a coarse clock).
fn tempfile(contents: &str) -> (String, std::fs::File) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut path = std::env::temp_dir();
    path.push(format!("neoc_e2e_{}_{n}.luau", std::process::id()));
    let mut file = std::fs::File::create(&path).expect("create temp file");
    file.write_all(contents.as_bytes())
        .expect("write temp file");
    (path.to_string_lossy().into_owned(), file)
}

/// Transpile, append a call to the entry function, run, and return stdout.
fn transpile_and_run(source: &str, entry: &str) -> Option<String> {
    let out = neoc_compiler::transpile(source, neoc_compiler::Target::Luau);
    assert!(out.ok(), "unexpected diagnostics: {:#?}", out.diagnostics);
    let program = format!("{}\n{entry}()\n", out.output);
    run_luau(&program)
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
    let Some(stdout) = transpile_and_run(src, "main") else {
        eprintln!("skipping: no luau interpreter on PATH");
        return;
    };
    assert_eq!(stdout.trim(), "55");
}

#[test]
fn loops_and_accumulation_run() {
    let src = "\
extern { fn print(n: number); }
fn sum_to(n: number) -> number {
  let mut total = 0;
  for i in 1..n { total = total + i; }
  total
}
fn main() {
  print(sum_to(11));
}";
    let Some(stdout) = transpile_and_run(src, "main") else {
        eprintln!("skipping: no luau interpreter on PATH");
        return;
    };
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
    let Some(stdout) = transpile_and_run(src, "main") else {
        eprintln!("skipping: no luau interpreter on PATH");
        return;
    };
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
    let Some(stdout) = transpile_and_run(src, "main") else {
        eprintln!("skipping: no luau interpreter on PATH");
        return;
    };
    assert_eq!(stdout.trim(), "square");
}

#[test]
fn continue_and_break_run() {
    // `continue` skips i == 3; `break` exits at i == 7. The sum is
    // 0+1+2+4+5+6 = 18. Exercises the repeat/break restructuring under Lua 5.1.
    let src = "\
extern { fn print(n: number); }
fn main() {
  let mut acc = 0;
  for i in 0..10 {
    if i == 3 { continue }
    if i == 7 { break }
    acc = acc + i;
  }
  print(acc);
}";
    let Some(stdout) = transpile_and_run(src, "main") else {
        eprintln!("skipping: no luau interpreter on PATH");
        return;
    };
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
    let Some(stdout) = transpile_and_run(src, "main") else {
        eprintln!("skipping: no luau interpreter on PATH");
        return;
    };
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
    let Some(stdout) = transpile_and_run(src, "main") else {
        eprintln!("skipping: no luau interpreter on PATH");
        return;
    };
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
    let Some(stdout) = transpile_and_run(src, "main") else {
        eprintln!("skipping: no luau interpreter on PATH");
        return;
    };
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
    let Some(stdout) = transpile_and_run(src, "main") else {
        eprintln!("skipping: no luau interpreter on PATH");
        return;
    };
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
    let Some(stdout) = transpile_and_run(src, "main") else {
        eprintln!("skipping: no luau interpreter on PATH");
        return;
    };
    assert_eq!(stdout.trim(), "true\ntrue");
}
