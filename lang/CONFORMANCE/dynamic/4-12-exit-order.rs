#![allow(unused_variables)]

struct Pair {
    a: i64,
    b: i64,
}

fn mark(s: &str) -> i64 {
    println!("{}", s);
    1
}

fn add(a: i64, b: i64) -> i64 {
    a + b
}

fn call_args(stop: bool) -> String {
    let n = add(mark("left"), {
        if stop {
            return "stopped".to_string();
        }
        mark("right")
    });
    (if n == 2 { "sum" } else { "?" }).to_string()
}

fn fields(stop: bool) -> String {
    let p = Pair {
        a: mark("a"),
        b: {
            if stop {
                return "stopped".to_string();
            }
            mark("b")
        },
    };
    (if p.a + p.b == 2 { "pair" } else { "?" }).to_string()
}

fn lazy_and(a: bool) -> String {
    let ok = a && {
        if a {
            return "rhs ran".to_string();
        }
        true
    };
    (if ok { "true" } else { "false" }).to_string()
}

fn lazy_or(a: bool) -> String {
    let ok = a || {
        if !a {
            return "rhs ran".to_string();
        }
        true
    };
    (if ok { "true" } else { "false" }).to_string()
}

fn main() {
    println!("{}", call_args(false));
    println!("{}", call_args(true));
    println!("{}", fields(false));
    println!("{}", fields(true));
    println!("{}", lazy_and(false));
    println!("{}", lazy_and(true));
    println!("{}", lazy_or(true));
    println!("{}", lazy_or(false));
}
