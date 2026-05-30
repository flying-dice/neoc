//! End-to-end behavioural oracle: emit Python for a real `.neoc` program, write
//! it next to the shared `neoc_lib.py`, and **run it under a real `python3`**
//! (3.10+, for `match`/`case`). The conformance contract is observable
//! behaviour — the JS backend runs `node`; this is the Python analogue. It
//! proves a `Result`-returning fn, a native `match`, a `struct` method, and a
//! `?` chain execute and produce the expected stdout.
//!
//! Gated behind a 3.10+ interpreter being on `PATH`: when none is found the test
//! prints a skip notice and passes (CI without Python must not go red), mirroring
//! how optional-toolchain oracles are guarded elsewhere.

use std::process::Command;

/// The neoc program under test: a struct with a method, an enum matched
/// natively, a `Result`-returning fn, and a `?` chain through `Result`.
const SOURCE: &str = r#"
pub struct Point { x: number, y: number }

impl Point {
    fn sum(self) -> number { self.x + self.y }
}

pub enum Shape {
    Circle(number),
    Rect { w: number, h: number },
}

pub fn area(s: Shape) -> number {
    match s {
        Circle(r) => (3 * r) * r,
        Rect { w, h } => w * h,
    }
}

pub fn checked_div(a: number, b: number) -> Result<number, string> {
    if b == 0 {
        Err("divide by zero")
    } else {
        Ok(a / b)
    }
}

pub fn pipeline() -> Result<number, string> {
    let q = checked_div(10, 2)?;
    Ok(q + 1)
}
"#;

/// A driver that exercises the emitted module and prints one line per check, so
/// the test asserts on deterministic stdout.
const DRIVER: &str = r"
from sample import Point, Shape, Circle, Rect, area, checked_div, pipeline

print(int(Point(x=1, y=2).sum()))
print(int(area(Circle(2))))
print(int(area(Rect(w=3, h=4))))
r = checked_div(10, 2)
print(r.is_ok(), int(r.unwrap()))
e = checked_div(1, 0)
print(e.is_err(), e.unwrap_err())
p = pipeline()
print(p.is_ok(), int(p.unwrap()))
";

/// Find a Python interpreter that is at least 3.10 (needed for `match`/`case`).
fn find_python_3_10() -> Option<String> {
    for cand in [
        "python3.14",
        "python3.13",
        "python3.12",
        "python3.11",
        "python3.10",
        "python3",
        "python",
    ] {
        let Ok(out) = Command::new(cand)
            .args([
                "-c",
                "import sys; print(sys.version_info[0], sys.version_info[1])",
            ])
            .output()
        else {
            continue;
        };
        if !out.status.success() {
            continue;
        }
        let v = String::from_utf8_lossy(&out.stdout);
        let mut it = v.split_whitespace();
        let (Some(maj), Some(min)) = (it.next(), it.next()) else {
            continue;
        };
        if maj == "3" && min.parse::<u32>().is_ok_and(|m| m >= 10) {
            return Some(cand.to_string());
        }
    }
    None
}

#[test]
fn emitted_python_runs_under_python3() {
    let Some(python) = find_python_3_10() else {
        eprintln!("skipping: no python3 >= 3.10 found on PATH");
        return;
    };

    let parsed = neoc_parser::parse(SOURCE);
    assert!(
        parsed.diagnostics.is_empty(),
        "fixture should parse cleanly: {:?}",
        parsed.diagnostics
    );
    let result = neoc_codegen_python::emit(&parsed.module, &[]);
    assert!(
        result.diagnostics.is_empty(),
        "emit should be clean: {:?}",
        result.diagnostics
    );

    // A unique temp dir holding the package: `neoc_lib.py`, the emitted module,
    // its `.pyi`, and the driver. Relative imports need a package, so the driver
    // runs with the temp dir on `sys.path` (it is, as the script's own dir).
    let dir = std::env::temp_dir().join(format!("neoc_py_run_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");

    // The emitted module uses `from .neoc_lib import …`, so it must live inside a
    // package. Make the temp dir a package and import via the package name.
    let pkg = dir.join("genpkg");
    std::fs::create_dir_all(&pkg).expect("create package dir");
    std::fs::write(pkg.join("__init__.py"), "").expect("write __init__");
    std::fs::write(pkg.join("neoc_lib.py"), neoc_codegen_python::neoc_lib_py())
        .expect("write neoc_lib.py");
    std::fs::write(
        pkg.join("neoc_lib.pyi"),
        neoc_codegen_python::neoc_lib_pyi(),
    )
    .expect("write neoc_lib.pyi");
    std::fs::write(pkg.join("sample.py"), &result.py).expect("write sample.py");
    std::fs::write(pkg.join("sample.pyi"), &result.pyi).expect("write sample.pyi");
    // The driver imports from the package, so rewrite its bare import.
    let driver = DRIVER.replace("from sample import", "from genpkg.sample import");
    std::fs::write(dir.join("driver.py"), driver).expect("write driver.py");

    let out = Command::new(&python)
        .arg("driver.py")
        .current_dir(&dir)
        .output()
        .expect("run python driver");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "python run failed ({python}):\nstdout:\n{stdout}\nstderr:\n{stderr}\n--- sample.py ---\n{}",
        result.py
    );

    let expected = "3\n12\n12\nTrue 5\nTrue divide by zero\nTrue 6\n";
    assert_eq!(
        stdout, expected,
        "unexpected stdout from emitted Python:\n--- sample.py ---\n{}",
        result.py
    );

    let _ = std::fs::remove_dir_all(&dir);
}
