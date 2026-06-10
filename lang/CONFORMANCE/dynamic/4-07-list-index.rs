fn main() {
    let xs: Vec<i64> = vec![5, 6, 7, 8];
    println!("{}", xs[0]);
    println!("{}", xs[3]);

    let i: i64 = 1;
    println!("{}", xs[i as usize]);
    println!("{}", xs[(i + 1) as usize]);
}
