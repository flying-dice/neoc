#![allow(unused)]
fn clamp(n: f64) -> f64 {
    if n < 0.0 {
        return 0.0;
    }
    if n > 100.0 {
        return 100.0;
    }
    n
}

fn main() {
    println!("{}", clamp(-5.0));
    println!("{}", clamp(250.0));
    println!("{}", clamp(42.0));
}
