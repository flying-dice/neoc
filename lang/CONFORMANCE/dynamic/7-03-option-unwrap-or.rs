fn main() {
    let s = Some(7.0);
    let n: Option<f64> = None;
    println!("{}", s.unwrap_or(0.0));
    println!("{}", n.unwrap_or(99.0));
}
