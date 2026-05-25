fn main() {
    let mut acc: i64 = 0;
    for i in 0..10 {
        if i == 3 {
            continue;
        }
        if i == 7 {
            break;
        }
        acc = acc + i;
    }
    println!("{}", acc);
}
