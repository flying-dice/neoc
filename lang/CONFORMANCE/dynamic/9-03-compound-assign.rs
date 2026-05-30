#![allow(unused)]
fn main() {
    let mut a = 10.0;
    a += 5.0;
    a -= 3.0;
    a *= 2.0;
    println!("{}", a);

    let mut b = 10.0;
    b /= 4.0;
    println!("{}", b);

    let mut c = 17.0;
    c %= 5.0;
    println!("{}", c);
}
