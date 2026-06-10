fn main() {
    let half: Vec<i64> = (1..4).collect();
    let incl: Vec<i64> = (1..=4).collect();
    let mut sum: i64 = 0;
    for x in half {
        sum = sum + x;
    }
    for y in incl {
        sum = sum + y;
    }
    println!("{}", sum);
}
