fn main() {
    let s = Some(4.0);
    match s.map(|x| x * 2.0) {
        Some(v) => println!("{}", v),
        None => println!("{}", 0.0),
    }
}
