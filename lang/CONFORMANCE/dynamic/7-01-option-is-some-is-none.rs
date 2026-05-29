fn main() {
    let s = Some(1.0);
    let n: Option<f64> = None;
    println!("{}", s.is_some());
    println!("{}", s.is_none());
    println!("{}", n.is_some());
    println!("{}", n.is_none());
}
