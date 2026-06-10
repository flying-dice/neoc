fn main() {
    let stepped = |n: f64| {
        let doubled = n * 2.0;
        doubled + 1.0
    };
    println!("{}", stepped(20.0));
    println!("{}", stepped(0.0));
}
