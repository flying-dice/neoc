#![allow(unused)]
fn main() {
    let mut acc = 0.0;
    for i in 0..3 {
        for j in 0..3 {
            if j == 1 {
                continue;
            }
            if j == 2 {
                break;
            }
            acc = acc + (i as f64) * 10.0 + (j as f64);
        }
    }
    println!("{}", acc);
}
