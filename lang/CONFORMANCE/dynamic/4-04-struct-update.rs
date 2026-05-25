struct Point {
    x: f64,
    y: f64,
}

fn main() {
    let p = Point { x: 1.0, y: 2.0 };
    let q = Point { x: 9.0, ..p };
    println!("{}", q.x);
    println!("{}", q.y);
}
