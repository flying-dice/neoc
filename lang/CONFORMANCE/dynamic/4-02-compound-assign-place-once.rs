struct P {
    x: f64,
    name: String,
}

struct N(f64);

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

fn pick<'a>(ps: &'a mut Vec<P>, calls: &mut Vec<String>) -> &'a mut P {
    calls[0] += "p";
    &mut ps[0]
}

fn pick_ok<'a>(ns: &'a mut Vec<N>, calls: &mut Vec<String>) -> Result<&'a mut N, String> {
    calls[0] += "n";
    Ok(&mut ns[0])
}

fn fields(calls: &mut Vec<String>) -> Result<String, String> {
    let mut ps = vec![P { x: 1.0, name: "a".to_string() }];
    pick(&mut ps, calls).x += 5.0;
    pick(&mut ps, calls).name += "!";
    let mut ns = vec![N(1.0)];
    pick_ok(&mut ns, calls)?.0 += 10.0;
    if ps[0].x == 6.0 && ns[0].0 == 11.0 {
        Ok(ps[0].name.clone())
    } else {
        Err("field place evaluated twice".to_string())
    }
}

fn show(r: Result<String, String>) -> String {
    match r {
        Ok(v) => v,
        Err(e) => e,
    }
}

fn main() {
    let mut calls = vec![String::new()];
    println!("{}", show(run(&mut calls)));
    println!("{}", show(fields(&mut calls)));
    println!("{}", calls[0]);
}
