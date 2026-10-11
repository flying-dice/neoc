fn report(n: i64) {
    if n > 1 {
        println!("high");
        return;
    }
    println!("low");
}

fn main() {
    report(0);
    report(2);
}
