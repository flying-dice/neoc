enum Shape {
    Rect { w: i64, h: i64 },
    Dot,
}

fn area(s: Shape) -> i64 {
    match s {
        Shape::Rect { w, h } => w * h,
        Shape::Dot => 0,
    }
}

fn main() {
    println!("{}", area(Shape::Rect { w: 3, h: 4 }));
    println!("{}", area(Shape::Dot));
}
