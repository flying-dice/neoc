#[derive(Clone)]
struct Inner {
    v: f64,
}

impl Inner {
    fn double(&self) -> f64 {
        self.v * 2.0
    }
}

#[derive(Clone)]
enum Shape {
    Dot,
    Boxed(Inner),
}

impl Shape {
    fn weight(&self) -> f64 {
        match self {
            Shape::Dot => 0.0,
            Shape::Boxed(i) => i.v,
        }
    }
}

#[derive(Clone)]
struct Outer {
    i: Inner,
    o: Option<f64>,
    xs: Vec<f64>,
    s: Shape,
}

fn main() {
    let a = Outer { i: Inner { v: 4.0 }, o: Some(5.0), xs: vec![6.0, 7.0], s: Shape::Boxed(Inner { v: 8.0 }) };
    let b = a.clone();
    println!("{}", b.i.double());
    println!("{}", b.o.unwrap());
    println!("{}", b.xs[1]);
    println!("{}", b.s.weight());
    let d = Shape::Dot.clone();
    println!("{}", d.weight());
    let e = Shape::Boxed(Inner { v: 9.0 }).clone();
    println!("{}", e.weight());
    let n: Option<f64> = None;
    let c = Outer { i: Inner { v: 1.0 }, o: n, xs: vec![], s: Shape::Dot };
    let c2 = c.clone();
    if c2.o.is_none() {
        println!("{}", 1.0);
    }
}
