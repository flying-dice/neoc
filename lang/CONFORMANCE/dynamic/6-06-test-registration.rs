// neoc's `#[test]` (DYNAMIC.md §6.5) lowers the fn normally and registers it on
// the module `__tests` table — verified by the emit golden, not stdout. The
// registration has no runtime-observable effect in a plain Rust binary, so this
// anchor omits `#[test]` and mirrors only the stdout the two fns produce when
// main calls them, just as the derive cases hand-write neoc's output bytes.
fn checks_alpha() {
    println!("alpha-ran");
}

fn checks_beta() {
    println!("beta-ran");
}

fn main() {
    checks_alpha();
    checks_beta();
}
