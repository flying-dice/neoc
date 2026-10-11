fn tick(calls: &mut Vec<String>) -> Result<usize, String> {
    calls[0] += "t";
    Ok(0)
}

fn run(calls: &mut Vec<String>) -> Result<String, String> {
    let mut xs = vec!["a".to_string(), "b".to_string()];
    xs[tick(calls)?] += "!";
    let mut ns = vec![1.0, 2.0];
    ns[tick(calls)?] += 10.0;
    if ns[0] == 11.0 {
        Ok(xs[0].clone() + &xs[1])
    } else {
        Err("numeric place evaluated twice".to_string())
    }
}

fn main() {
    let mut calls = vec![String::new()];
    match run(&mut calls) {
        Ok(v) => println!("{}", v),
        Err(e) => println!("{}", e),
    }
    println!("{}", calls[0]);
}
