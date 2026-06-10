struct Point {
    x: i64,
    y: i64,
    #[allow(dead_code)]
    z: i64,
}

fn classify(p: Point) -> i64 {
    match p {
        Point { x: 0, y, .. } => y,
        Point { x, .. } => x,
    }
}

fn main() {
    println!("{}", classify(Point { x: 0, y: 7, z: 99 }));
    println!("{}", classify(Point { x: 5, y: 8, z: 99 }));
}
