struct Point {
    x: f64,
    y: f64,
}

impl Point {
    fn origin() -> Self {
        Self { x: 0.0, y: 0.0 }
    }

    fn at(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    fn with_x(self, x: f64) -> Self {
        Self { x, ..self }
    }
}

fn main() {
    let o = Point::origin();
    let p = Point::at(3.0, 4.0).with_x(9.0);
    println!("{}", o.x);
    println!("{}", p.x);
    println!("{}", p.y);
}
