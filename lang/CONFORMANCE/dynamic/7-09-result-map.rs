fn read(r: Result<f64, &str>) -> f64 {
    match r {
        Ok(v) => v,
        Err(_) => 0.0,
    }
}

fn main() {
    let ok: Result<f64, &str> = Ok(10.0);
    let err: Result<f64, &str> = Err("e");
    println!("{}", read(ok.map(|x| x + 5.0)));
    println!("{}", read(err.map(|x| x + 5.0)));
}
