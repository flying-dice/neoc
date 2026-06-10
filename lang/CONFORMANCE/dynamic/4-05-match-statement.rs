fn classify(n: i64) {
    match n {
        0 => println!("zero"),
        1 => println!("one"),
        _ => println!("many"),
    }
}

fn main() {
    classify(0);
    classify(1);
    classify(7);
}
