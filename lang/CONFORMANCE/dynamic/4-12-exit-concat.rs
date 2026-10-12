fn append_break(n: i64) -> String {
    let mut s = String::new();
    for i in 0..n {
        s = format!("{}{}{}", s, "a", {
            if i == 1 {
                break;
            }
            "b"
        });
    }
    s
}

fn append_continue(n: i64) -> String {
    let mut s = String::new();
    for i in 0..n {
        s = format!("{}{}{}", s, "a", {
            if i == 1 {
                continue;
            }
            "b"
        });
    }
    s
}

fn main() {
    println!("{}", append_break(3));
    println!("{}", append_continue(3));
}
