fn classify(n: f64) -> &'static str {
    match n {
        n if n == 0.0 => "zero",
        n if n == 1.0 || n == 2.0 || n == 3.0 => "small",
        x if x > 100.0 => "huge",
        _ => "other",
    }
}

fn main() {
    println!("{}", classify(0.0));
    println!("{}", classify(2.0));
    println!("{}", classify(200.0));
    println!("{}", classify(50.0));
}
