// neoc's derived `Display` joins the field values with ", " and no type-name
// decoration, printing each `number` (an `f64`) through Lua `tostring` (no
// decimal point for an integral value). The closest Rust anchor is a
// hand-written `Display` matching neoc's bytes.
use std::fmt;

struct Point {
    x: f64,
    y: f64,
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}, {}", self.x, self.y)
    }
}

fn main() {
    let p = Point { x: 1.0, y: 2.0 };
    println!("{}", p);
}
