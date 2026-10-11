#![allow(unused_variables)]

fn step(i: i64, limit: i64) -> Result<i64, String> {
    if i == limit {
        Err("stop".to_string())
    } else {
        Ok(i)
    }
}

fn in_block(limit: i64) -> String {
    let mut s = String::new();
    let tag = {
        for i in 0..3 {
            s += "x";
            if i == limit {
                return "early".to_string();
            }
        }
        "done"
    };
    s + "|" + tag
}

fn in_if(limit: i64) -> String {
    let mut s = String::new();
    let tag = if limit >= 0 {
        for i in 0..3 {
            s += "y";
            if i == limit {
                return s;
            }
        }
        "done"
    } else {
        "neg"
    };
    s + "|" + tag
}

fn in_try(limit: i64) -> Result<String, String> {
    let mut s = String::new();
    let tag = {
        for i in 0..3 {
            s += "z";
            let n = step(i, limit)?;
        }
        "done"
    };
    Ok(s + "|" + tag)
}

fn show(r: Result<String, String>) -> String {
    match r {
        Ok(v) => v,
        Err(e) => e,
    }
}

fn main() {
    println!("{}", in_block(99));
    println!("{}", in_if(99));
    println!("{}", in_if(-1));
    println!("{}", show(in_try(99)));
}
