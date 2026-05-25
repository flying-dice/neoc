fn incr(o: Option<f64>) -> Option<f64> {
    let n = o?;
    Some(n + 1.0)
}

fn unwrap_or(o: Option<f64>) -> f64 {
    match o {
        Some(n) => n,
        None => 0.0,
    }
}

fn main() {
    println!("{}", unwrap_or(incr(Some(41.0))));
    println!("{}", unwrap_or(incr(None)));
}
