fn step(r: Result<f64, &str>) -> Result<f64, &str> {
    let v = r.map(|x| x + 10.0)?;
    Ok(v)
}

fn read(r: Result<f64, &str>) -> f64 {
    match r {
        Ok(v) => v,
        Err(_) => 0.0,
    }
}

fn main() {
    println!("{}", read(step(Ok(5.0))));
    println!("{}", read(step(Err("nope"))));
}
