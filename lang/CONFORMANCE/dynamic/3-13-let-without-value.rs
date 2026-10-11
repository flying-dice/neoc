fn pick(big: bool) -> i64 {
    let r: i64;
    if big {
        r = 100;
    } else {
        r = 1;
    }
    r
}

fn main() {
    println!("{}", pick(true));
    println!("{}", pick(false));
}
