fn read_err(r: Result<f64, &str>) -> &str {
    match r {
        Ok(_) => "ok",
        Err(e) => e,
    }
}

fn main() {
    let ok: Result<f64, &str> = Ok(1.0);
    let err: Result<f64, &str> = Err("x");
    println!("{}", read_err(ok.map_err(|e| e)));
    println!("{}", read_err(err.map_err(|e| e)));
}
