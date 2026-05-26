trait Drawable {
    fn draw(&self) -> String;
}

struct Circle {
    #[allow(dead_code)]
    r: f64,
}

struct Square {
    #[allow(dead_code)]
    side: f64,
}

impl Drawable for Circle {
    fn draw(&self) -> String {
        "O".to_string()
    }
}

impl Drawable for Square {
    fn draw(&self) -> String {
        "[]".to_string()
    }
}

fn render(shape: &dyn Drawable) -> String {
    shape.draw()
}

fn main() {
    println!("{}", render(&Circle { r: 1.0 }));
    println!("{}", render(&Square { side: 2.0 }));
}
