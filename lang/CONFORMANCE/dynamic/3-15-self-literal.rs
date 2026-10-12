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

    fn diagonal(n: f64) -> Self {
        let make = |k: f64| Self { x: k, y: k };
        make(n * 2.0)
    }
}

trait Make {
    fn make() -> Self;
}

impl Make for Point {
    fn make() -> Self {
        Self { x: 42.0, y: 0.0 }
    }
}

struct Wrap<T> {
    v: T,
    n: f64,
}

impl<T> Wrap<T> {
    fn of(v: T) -> Wrap<T> {
        Self { v, n: 1.0 }
    }

    fn with(self, v: T) -> Wrap<T> {
        Self { v, ..self }
    }
}

fn main() {
    let o = Point::origin();
    let p = Point::at(3.0, 4.0).with_x(9.0);
    println!("{}", o.x);
    println!("{}", p.x);
    println!("{}", p.y);
    println!("{}", Point::diagonal(4.0).y);
    println!("{}", Point::make().x);
    let w = Wrap::of(5.0).with(7.0);
    println!("{}", w.v);
    println!("{}", w.n);
}
