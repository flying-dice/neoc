fn yes_no(b: bool) -> &'static str {
    match b {
        true => "yes",
        false => "no",
    }
}

fn main() {
    println!("{}", yes_no(true));
    println!("{}", yes_no(false));
}
