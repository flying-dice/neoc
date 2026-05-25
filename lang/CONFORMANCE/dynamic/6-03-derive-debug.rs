// neoc's derived `Debug` renders `Name { field = value }` with `=` separators
// and prints each `number` (an `f64`) through Lua `tostring`, which shows an
// integral value with no decimal point. Rust's `{:?}` uses `:` separators and
// `1.0`, so the closest anchor is a hand-written `debug` matching neoc's bytes.
struct Point {
    x: f64,
    y: f64,
}

impl Point {
    fn debug(&self) -> String {
        format!("Point {{ x = {}, y = {} }}", self.x, self.y)
    }
}

fn main() {
    let p = Point { x: 1.0, y: 2.0 };
    println!("{}", p.debug());
}
