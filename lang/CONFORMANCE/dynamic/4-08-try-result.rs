#![allow(unused_variables)]

fn parse(ok: bool) -> Result<f64, String> {
    if ok {
        Ok(10.0)
    } else {
        Err("bad".to_string())
    }
}

fn doubled(ok: bool) -> Result<f64, String> {
    let n = parse(ok)?;
    Ok(n + n)
}

fn show(r: Result<f64, String>) -> String {
    match r {
        Ok(n) => "ok".to_string(),
        Err(e) => e,
    }
}

fn main() {
    println!("{}", show(doubled(true)));
    println!("{}", show(doubled(false)));
}
