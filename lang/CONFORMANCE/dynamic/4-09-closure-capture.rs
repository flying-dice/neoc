#![allow(unused)]
fn adder(base: f64) -> impl Fn(f64) -> f64 {
    move |x| x + base
}

fn main() {
    let add10 = adder(10.0);
    println!("{}", add10(5.0));
    println!("{}", add10(32.0));
}
