fn main() {
    let mut xs: Vec<i64> = vec![1, 2, 3];
    xs[0] += 10;
    xs[0] *= 2;
    xs[1] *= 5;
    let i: i64 = 2;
    xs[i as usize] -= 1;
    println!("{}", xs[0]);
    println!("{}", xs[1]);
    println!("{}", xs[2]);
}
