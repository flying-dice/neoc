#![allow(unused)]
fn dig(o: Option<Result<f64, String>>) -> f64 {
    match o {
        Some(Ok(n)) => n,
        Some(Err(_)) => -1.0,
        None => 0.0,
    }
}

fn main() {
    println!("{}", dig(Some(Ok(42.0))));
    println!("{}", dig(Some(Err("boom".to_string()))));
    println!("{}", dig(None));
}
