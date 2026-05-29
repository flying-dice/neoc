fn main() {
    let ok: Result<f64, &str> = Ok(33.0);
    println!("{}", ok.unwrap());
}
