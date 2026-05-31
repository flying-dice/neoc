#![allow(unused)]
fn loud(tag: &str) -> bool {
    println!("{}", tag);
    true
}

fn main() {
    let _ = false && loud("and-skipped");
    let _ = true || loud("or-skipped");
    let _ = true && loud("and-run");
    let _ = false || loud("or-run");
}
