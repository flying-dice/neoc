#![allow(unused_variables)]

fn describe(o: Option<f64>) -> &'static str {
    match o {
        Some(n) => "some",
        None => "none",
    }
}

fn main() {
    println!("{}", describe(Some(3.0)));
    println!("{}", describe(None));
}
