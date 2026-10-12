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

struct Pt {
    x: i64,
    y: i64,
}

fn shorthand(stop: bool) -> String {
    let mut x = 1;
    let p = Pt {
        x,
        y: {
            if stop {
                return "stopped".to_string();
            }
            x += 10;
            2
        },
    };
    (if p.x == 1 { "bound" } else { "shifted" }).to_string()
}

fn range_bounds(stop: bool) -> String {
    let mut s = String::new();
    for i in mark("lo")..({
        if stop {
            return "stopped".to_string();
        }
        mark("hi") + 2
    }) {
        s += "r";
    }
    s
}

fn update(stop: bool) -> String {
    let p = Pt { x: 1, y: 2 };
    let q = Pt {
        y: mark("y"),
        ..({
            if stop {
                return "stopped".to_string();
            }
            p
        })
    };
    (if q.x == 1 { "kept" } else { "?" }).to_string()
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
    println!("{}", shorthand(false));
    println!("{}", shorthand(true));
    println!("{}", range_bounds(false));
    println!("{}", range_bounds(true));
    println!("{}", update(false));
    println!("{}", update(true));
}
