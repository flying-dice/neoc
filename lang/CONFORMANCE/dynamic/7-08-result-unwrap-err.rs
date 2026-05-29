fn main() {
    let err: Result<f64, &str> = Err("boom");
    println!("{}", err.unwrap_err());
}
