fn main() {
    let xs: Vec<i64> = vec![2, 4, 6, 8];
    let mut sum: i64 = 0;
    for x in &xs {
        sum = sum + x;
    }
    println!("{}", sum);

    let mut acc: i64 = 0;
    for x in &xs {
        acc = acc * 10 + x;
    }
    println!("{}", acc);
}
