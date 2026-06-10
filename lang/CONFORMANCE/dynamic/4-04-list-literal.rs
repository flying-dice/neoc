fn main() {
    let xs: Vec<i64> = vec![10, 20, 30];
    println!("{}", xs[0]);
    println!("{}", xs[1]);
    println!("{}", xs[2]);

    let base: i64 = 5;
    let ys: Vec<i64> = vec![base, base + 1, base * 2];
    println!("{}", ys[0]);
    println!("{}", ys[1]);
    println!("{}", ys[2]);
}
