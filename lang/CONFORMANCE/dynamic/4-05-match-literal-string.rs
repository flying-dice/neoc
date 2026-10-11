fn code(word: &str) -> i64 {
    match word {
        "yes" => 1,
        "no" => 2,
        _ => 0,
    }
}

fn main() {
    println!("{}", code("yes"));
    println!("{}", code("no"));
    println!("{}", code("maybe"));
}
