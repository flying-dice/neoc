#![allow(unused)]
fn grade(n: f64) -> String {
    if n >= 90.0 {
        "A".to_string()
    } else if n >= 80.0 {
        "B".to_string()
    } else if n >= 70.0 {
        "C".to_string()
    } else {
        "F".to_string()
    }
}

fn main() {
    println!("{}", grade(95.0));
    println!("{}", grade(82.0));
    println!("{}", grade(40.0));
}
