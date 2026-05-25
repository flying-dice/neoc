// neoc models `number` as `f64`, which Rust's `Eq` cannot derive (floats are
// only `PartialEq`); `PartialEq` is the closest anchor and gives the same `==`
// / `!=` structural comparison this case exercises.
#[derive(PartialEq)]
struct Pair {
    a: f64,
    b: f64,
}

fn main() {
    let p = Pair { a: 1.0, b: 2.0 };
    let q = Pair { a: 1.0, b: 2.0 };
    let r = Pair { a: 9.0, b: 2.0 };
    if p == q {
        println!("eq");
    }
    if p != r {
        println!("ne");
    }
}
