fn main() {
    let mut s = String::new();
    while s != "xxx" {
        s += "x";
    }
    println!("{}", s);

    let mut t = String::new();
    for _i in 0..3 {
        t = t + "y";
        println!("{}", t);
    }
}
