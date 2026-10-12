#![allow(unused_variables)]

fn in_block(limit: i64) -> String {
    let mut s = String::new();
    let tag = {
        for i in 0..5 {
            s += "x";
            if i == limit {
                return "early".to_string();
            }
        }
        "done"
    };
    s + "|" + tag
}

fn in_if(early: bool) -> String {
    let tag = if early { return "early".to_string() } else { "done" };
    "|".to_string() + tag
}

fn in_match(r: Result<String, String>) -> Result<String, String> {
    let v = match r {
        Ok(v) => v,
        Err(e) => return Err(e),
    };
    Ok("|".to_string() + &v)
}

fn nested(n: i64) -> String {
    let r = if n > 0 {
        {
            if n > 5 {
                return "big".to_string();
            }
            "small"
        }
    } else {
        "none"
    };
    "|".to_string() + r
}

fn closure_return(n: i64) -> String {
    let f = |y: i64| {
        if y > 0 {
            return "pos";
        }
        "other"
    };
    f(n).to_string() + "|" + "after"
}

fn show(r: Result<String, String>) -> String {
    match r {
        Ok(v) => v,
        Err(e) => e,
    }
}

fn main() {
    println!("{}", in_block(1));
    println!("{}", in_block(9));
    println!("{}", in_if(true));
    println!("{}", in_if(false));
    println!("{}", show(in_match(Err("bad".to_string()))));
    println!("{}", show(in_match(Ok("good".to_string()))));
    println!("{}", nested(9));
    println!("{}", nested(2));
    println!("{}", nested(0));
    println!("{}", closure_return(1));
    println!("{}", closure_return(0));
}
