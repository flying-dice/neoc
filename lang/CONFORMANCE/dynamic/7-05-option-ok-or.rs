fn main() {
    let s = Some(5.0);
    let n: Option<f64> = None;
    println!("{}", s.ok_or("x").is_ok());
    println!("{}", n.ok_or("x").is_err());
}
