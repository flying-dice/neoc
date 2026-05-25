#[derive(Clone)]
struct Counter {
    value: f64,
}

fn main() {
    let a = Counter { value: 5.0 };
    let b = a.clone();
    println!("{}", a.value);
    println!("{}", b.value);
}
