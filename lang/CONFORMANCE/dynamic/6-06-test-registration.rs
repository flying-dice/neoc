// neoc's `#[test]` (DYNAMIC.md §6.5) lowers the fn normally and registers it on
// the module `__tests` table; the conformance runtime (§7) walks that table and
// runs each registered fn, so the neoc backends produce this stdout from the
// registration alone — drop a registration and the test stops running. A plain
// Rust binary has no such runtime table, so this anchor omits `#[test]` and calls
// both fns from `main` directly to reproduce the same stdout, just as the derive
// cases hand-write neoc's output bytes.
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
