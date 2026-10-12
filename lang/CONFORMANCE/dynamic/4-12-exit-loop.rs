#![allow(unused_variables)]

fn skip(n: i64) -> String {
    let mut s = String::new();
    let mut i = 0;
    while i < n {
        i += 1;
        let v = {
            if i == 2 {
                continue;
            }
            if i == 4 {
                break;
            }
            i
        };
        s += "a";
    }
    s
}

fn deep(n: i64) -> String {
    let mut s = String::new();
    let mut i = 0;
    loop {
        i += 1;
        let a = if i < n {
            {
                if i == 3 {
                    break;
                }
                1
            }
        } else {
            if true {
                break;
            }
            0
        };
        s += "t";
    }
    s
}

fn inner_loop(n: i64) -> String {
    let mut s = String::new();
    for i in 0..n {
        let v = {
            let mut t = 0;
            while true {
                t += 1;
                if t > 1 {
                    break;
                }
            }
            t
        };
        s += "b";
    }
    s
}

fn for_head(k: i64) -> String {
    let mut s = String::new();
    for x in 0..4 {
        for y in ({
            if x == k {
                continue;
            }
            vec![10, 20]
        }) {
            s += "y";
        }
    }
    s
}

fn main() {
    println!("{}", skip(9));
    println!("{}", skip(3));
    println!("{}", deep(10));
    println!("{}", deep(2));
    println!("{}", inner_loop(3));
    println!("{}", for_head(1));
}
