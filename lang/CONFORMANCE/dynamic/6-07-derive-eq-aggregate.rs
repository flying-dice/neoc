// neoc models `number` as `f64`, which Rust's `Eq` cannot derive; `PartialEq`
// gives the same structural `==` / `!=` this case exercises.
#[derive(PartialEq)]
struct Inner {
    v: f64,
}

#[derive(PartialEq)]
enum Shape {
    Dot,
    Boxed(Inner),
}

#[derive(PartialEq)]
struct Outer {
    i: Inner,
    o: Option<f64>,
    xs: Vec<f64>,
    s: Shape,
}

fn make(v: f64, o: Option<f64>, xs: Vec<f64>, s: Shape) -> Outer {
    Outer { i: Inner { v: v }, o: o, xs: xs, s: s }
}

fn main() {
    let base = make(1.0, Some(1.0), vec![1.0, 2.0], Shape::Boxed(Inner { v: 2.0 }));
    let same = make(1.0, Some(1.0), vec![1.0, 2.0], Shape::Boxed(Inner { v: 2.0 }));
    if base == same {
        println!("equal");
    }
    if base != make(9.0, Some(1.0), vec![1.0, 2.0], Shape::Boxed(Inner { v: 2.0 })) {
        println!("nested-ne");
    }
    if base != make(1.0, None, vec![1.0, 2.0], Shape::Boxed(Inner { v: 2.0 })) {
        println!("some-vs-none-ne");
    }
    if base != make(1.0, Some(7.0), vec![1.0, 2.0], Shape::Boxed(Inner { v: 2.0 })) {
        println!("option-payload-ne");
    }
    if base != make(1.0, Some(1.0), vec![1.0, 3.0], Shape::Boxed(Inner { v: 2.0 })) {
        println!("element-ne");
    }
    if base != make(1.0, Some(1.0), vec![1.0], Shape::Boxed(Inner { v: 2.0 })) {
        println!("length-ne");
    }
    if base != make(1.0, Some(1.0), vec![], Shape::Boxed(Inner { v: 2.0 })) {
        println!("empty-vs-nonempty-ne");
    }
    if base != make(1.0, Some(1.0), vec![1.0, 2.0], Shape::Dot) {
        println!("tag-ne");
    }
    if base != make(1.0, Some(1.0), vec![1.0, 2.0], Shape::Boxed(Inner { v: 3.0 })) {
        println!("enum-payload-ne");
    }
    let none = make(1.0, None, vec![], Shape::Dot);
    if none == make(1.0, None, vec![], Shape::Dot) {
        println!("none-empty-equal");
    }
}
