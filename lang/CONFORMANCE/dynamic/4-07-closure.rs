fn apply(f: fn(f64) -> f64, x: f64) -> f64 {
    f(x)
}

fn main() {
    let double = |n| n * 2.0;
    println!("{}", apply(double, 21.0));
}
