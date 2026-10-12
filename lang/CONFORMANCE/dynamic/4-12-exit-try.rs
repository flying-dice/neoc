#![allow(unused_variables)]

fn parse(n: i64) -> Result<i64, String> {
    if n < 0 {
        Err("neg".to_string())
    } else {
        Ok(n)
    }
}

fn in_block(a: i64, b: i64) -> Result<String, String> {
    let total = {
        let x = parse(a)?;
        let y = parse(b)?;
        x + y
    };
    Ok("sum".to_string())
}

fn in_match(a: i64) -> Result<String, String> {
    let r = match a {
        0 => "zero",
        _ => {
            let v = parse(a)?;
            if v > 100 {
                "big"
            } else {
                "ok"
            }
        }
    };
    Ok(r.to_string())
}

fn in_closure(a: i64) -> Result<String, String> {
    let f = |y: i64| -> Result<String, String> {
        let z = parse(y)?;
        Ok("closure".to_string())
    };
    let r = f(a)?;
    Ok(r + "|after")
}

fn in_cond(limit: i64) -> Result<String, String> {
    let mut i = 0;
    while parse(limit - i)? > 0 {
        i += 2;
    }
    Ok("done".to_string())
}

fn show(r: Result<String, String>) -> String {
    match r {
        Ok(v) => v,
        Err(e) => e,
    }
}

fn main() {
    println!("{}", show(in_block(1, 2)));
    println!("{}", show(in_block(1, -2)));
    println!("{}", show(in_block(-1, 2)));
    println!("{}", show(in_match(0)));
    println!("{}", show(in_match(5)));
    println!("{}", show(in_match(500)));
    println!("{}", show(in_match(-5)));
    println!("{}", show(in_closure(1)));
    println!("{}", show(in_closure(-1)));
    println!("{}", show(in_cond(3)));
    println!("{}", show(in_cond(4)));
}
