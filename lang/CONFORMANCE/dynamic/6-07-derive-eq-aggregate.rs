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

#[derive(PartialEq)]
struct Many {
    ys: Vec<Inner>,
    r: Result<f64, String>,
    oo: Option<Option<Inner>>,
}

#[derive(PartialEq)]
struct Wrap<T> {
    v: T,
}

// Equal modulo 10 — a hand-written `eq`, so `1` and `11` compare equal.
struct Mod {
    v: f64,
}

impl PartialEq for Mod {
    fn eq(&self, o: &Mod) -> bool {
        self.v % 10.0 == o.v % 10.0
    }
}

#[derive(PartialEq)]
struct HasMod {
    m: Mod,
}

fn many(y: f64, r: Result<f64, String>, oo: Option<Option<Inner>>) -> Many {
    Many { ys: vec![Inner { v: y }], r: r, oo: oo }
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
    let m = many(1.0, Ok(1.0), Some(Some(Inner { v: 1.0 })));
    if m == many(1.0, Ok(1.0), Some(Some(Inner { v: 1.0 }))) {
        println!("many-equal");
    }
    if m != many(2.0, Ok(1.0), Some(Some(Inner { v: 1.0 }))) {
        println!("list-of-struct-ne");
    }
    if m != many(1.0, Err("1".to_string()), Some(Some(Inner { v: 1.0 }))) {
        println!("ok-vs-err-ne");
    }
    if m != many(1.0, Ok(2.0), Some(Some(Inner { v: 1.0 }))) {
        println!("ok-payload-ne");
    }
    if m != many(1.0, Ok(1.0), Some(Some(Inner { v: 2.0 }))) {
        println!("nested-option-payload-ne");
    }
    if m != many(1.0, Ok(1.0), Some(None)) {
        println!("some-some-vs-some-none-ne");
    }
    let w = Wrap { v: Inner { v: 1.0 } };
    let w_same = Wrap { v: Inner { v: 1.0 } };
    let w_other = Wrap { v: Inner { v: 2.0 } };
    if w == w_same {
        println!("generic-struct-equal");
    }
    if w != w_other {
        println!("generic-struct-ne");
    }
    let n = Wrap { v: 5.0 };
    let n_same = Wrap { v: 5.0 };
    let n_other = Wrap { v: 6.0 };
    if n == n_same {
        println!("generic-number-equal");
    }
    if n != n_other {
        println!("generic-number-ne");
    }
    let h = HasMod { m: Mod { v: 1.0 } };
    let h_mod = HasMod { m: Mod { v: 11.0 } };
    let h_other = HasMod { m: Mod { v: 2.0 } };
    if h == h_mod {
        println!("hand-written-eq");
    }
    if h != h_other {
        println!("hand-written-ne");
    }
}
