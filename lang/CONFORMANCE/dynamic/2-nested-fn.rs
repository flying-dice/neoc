fn label() -> f64 {
    1.0
}

fn main() {
    println!("{}", label());
    println!("{}", fact(5.0));
    println!("{}", is_even(10.0));
    fn fact(n: f64) -> f64 {
        if n < 2.0 {
            1.0
        } else {
            n * fact(n - 1.0)
        }
    }
    fn is_even(n: f64) -> f64 {
        if n == 0.0 {
            1.0
        } else {
            is_odd(n - 1.0)
        }
    }
    fn is_odd(n: f64) -> f64 {
        if n == 0.0 {
            0.0
        } else {
            is_even(n - 1.0)
        }
    }
    fn label() -> f64 {
        2.0
    }
}
