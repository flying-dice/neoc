#![allow(dead_code, unused_variables)]

enum Shape {
    Dot,
    Move(f64, f64),
    Circle { radius: f64 },
}

fn describe(s: Shape) -> &'static str {
    match s {
        Shape::Dot => "dot",
        Shape::Move(a, b) => "move",
        Shape::Circle { radius } => "circle",
    }
}

fn main() {
    println!("{}", describe(Shape::Dot));
    println!("{}", describe(Shape::Move(1.0, 2.0)));
    println!("{}", describe(Shape::Circle { radius: 5.0 }));
}
