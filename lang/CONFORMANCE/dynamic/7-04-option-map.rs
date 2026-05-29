fn main() {
    let s = Some(20.0);
    let n: Option<f64> = None;
    println!("{}", s.map(|x| x + 1.0).unwrap_or(0.0));
    println!("{}", n.map(|x| x + 1.0).unwrap_or(0.0));
}
