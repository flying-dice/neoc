fn main() {
    let mut i = 0.0;
    let mut sum = 0.0;
    while i < 5.0 {
        sum = sum + i;
        i = i + 1.0;
    }
    println!("{}", sum);
}
