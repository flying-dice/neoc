fn main() {
    let mut s = String::new();
    let mut i = 0;
    loop {
        i += 1;
        if i == 2 {
            continue;
        }
        s += "x";
        if i == 5 {
            break;
        }
    }
    println!("{}", s);
    let mut w = String::from("w");
    let mut n = 0;
    while n < 3 {
        n += 1;
        w += "!";
    }
    println!("{}", w);
}
