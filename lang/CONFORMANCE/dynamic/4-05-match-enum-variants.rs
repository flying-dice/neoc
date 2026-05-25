enum Shape {
    Dot,
    Move(f64, f64),
    Box { w: f64, h: f64 },
}

fn area(s: Shape) -> f64 {
    match s {
        Shape::Move(dx, dy) => dx + dy,
        Shape::Box { w, h } => w * h,
        _ => 0.0,
    }
}

fn main() {
    println!("{}", area(Shape::Move(3.0, 4.0)));
    println!("{}", area(Shape::Box { w: 5.0, h: 6.0 }));
    println!("{}", area(Shape::Dot));
}
