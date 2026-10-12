fn base() -> f64 {
    100.0
}

fn main() {
    let f = |n: f64| {
        let r = n + 1.0;
        fn twice(k: f64) -> f64 {
            k * 2.0
        }
        twice(r)
    };
    println!("{}", f(4.0));
    let mut acc = 0.0;
    for i in 0..5 {
        fn inc(k: f64) -> f64 {
            k + base()
        }
        if i == 2 {
            continue;
        }
        acc = acc + inc(i as f64);
    }
    println!("{}", acc);
    let v = match acc > 0.0 {
        true => {
            fn neg(k: f64) -> f64 {
                0.0 - k
            }
            neg(acc)
        }
        false => 0.0,
    };
    println!("{}", v);
}
