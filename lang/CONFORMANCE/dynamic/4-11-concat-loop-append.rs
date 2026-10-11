fn digit(i: i64) -> String {
    if i == 0 { "a" } else if i == 1 { "b" } else { "c" }.to_string()
}

fn main() {
    let mut s = String::from("start:");
    for i in 0..5 {
        s = s + &digit(i % 3);
        s += "-";
        s = s + "(" + &digit(i % 2) + ")";
    }
    println!("{}", s);
    let mut t = String::from("same");
    for _ in 0..0 {
        t += "x";
    }
    println!("{}", t);
}
