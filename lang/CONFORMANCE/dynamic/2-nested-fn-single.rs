fn main() {
    fn sum(n: f64) -> f64 {
        if n == 0.0 {
            0.0
        } else {
            n + sum(n - 1.0)
        }
    }
    println!("{}", sum(4.0));
}
