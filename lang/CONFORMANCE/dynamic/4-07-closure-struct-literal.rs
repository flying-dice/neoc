struct Point {
    x: f64,
    y: f64,
}

enum Shape {
    Square { side: f64 },
}

fn main() {
    let mk = |n| Point { x: n, y: n };
    let p = mk(7.0);
    println!("{}", p.x);
    println!("{}", p.y);

    let sq = |n| Shape::Square { side: n };
    let s = sq(5.0);
    match s {
        Shape::Square { side } => println!("{}", side),
    }
}
