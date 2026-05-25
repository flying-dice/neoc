fn main() {
    let x = 7.0;
    let mut y = if x > 5.0 { x * 2.0 } else { 0.0 };
    y = y + 1.0;
    println!("{}", y);
}
