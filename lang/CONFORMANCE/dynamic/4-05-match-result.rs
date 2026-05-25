#![allow(unused_variables)]

fn describe(r: Result<f64, String>) -> String {
    match r {
        Ok(n) => "ok".to_string(),
        Err(e) => e,
    }
}

fn main() {
    println!("{}", describe(Ok(7.0)));
    println!("{}", describe(Err("boom".to_string())));
}
