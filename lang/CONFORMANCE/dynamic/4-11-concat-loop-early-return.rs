fn upto(limit: i64) -> String {
    let mut s = String::new();
    for i in 0..10 {
        s += "x";
        if i == limit {
            return s;
        }
    }
    s + "|end"
}

fn verdict(limit: i64) -> String {
    let mut s = String::new();
    for i in 0..10 {
        s += "y";
        if i == limit {
            return "early".to_string();
        }
    }
    s
}

fn main() {
    println!("{}", upto(2));
    println!("{}", upto(99));
    println!("{}", verdict(3));
    println!("{}", verdict(99));
}
