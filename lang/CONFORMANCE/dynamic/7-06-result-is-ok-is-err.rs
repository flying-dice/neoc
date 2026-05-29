fn main() {
    let ok: Result<f64, &str> = Ok(1.0);
    let err: Result<f64, &str> = Err("bad");
    println!("{}", ok.is_ok());
    println!("{}", ok.is_err());
    println!("{}", err.is_ok());
    println!("{}", err.is_err());
}
