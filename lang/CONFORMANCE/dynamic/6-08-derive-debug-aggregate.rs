// neoc's derived Debug uses `Name { f = v }` decoration and renders `number`
// without a trailing `.0`; neither matches Rust's `derive`d format, so every
// `Debug` here is hand-written to emit neoc's bytes. `Option` and `Vec` render
// through small helpers in neoc's `Some(1)` / `None` / `[1, 2]` shape.
use std::fmt;

struct Inner {
    v: f64,
}

enum Shape {
    Dot,
    Boxed(Inner),
}

struct WithInner {
    i: Inner,
}

struct WithOption {
    o: Option<f64>,
}

struct WithList {
    xs: Vec<f64>,
}

struct WithShape {
    s: Shape,
}

struct All {
    i: Inner,
    o: Option<f64>,
    xs: Vec<f64>,
    s: Shape,
}

fn option(o: &Option<f64>) -> String {
    match o {
        Some(v) => format!("Some({})", v),
        None => "None".to_string(),
    }
}

fn list(xs: &[f64]) -> String {
    let parts: Vec<String> = xs.iter().map(|x| x.to_string()).collect();
    format!("[{}]", parts.join(", "))
}

impl fmt::Debug for Inner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Inner {{ v = {} }}", self.v)
    }
}

impl fmt::Debug for Shape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Shape::Dot => write!(f, "Dot"),
            Shape::Boxed(i) => write!(f, "Boxed({:?})", i),
        }
    }
}

impl fmt::Debug for WithInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WithInner {{ i = {:?} }}", self.i)
    }
}

impl fmt::Debug for WithOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WithOption {{ o = {} }}", option(&self.o))
    }
}

impl fmt::Debug for WithList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WithList {{ xs = {} }}", list(&self.xs))
    }
}

impl fmt::Debug for WithShape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WithShape {{ s = {:?} }}", self.s)
    }
}

impl fmt::Debug for All {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "All {{ i = {:?}, o = {}, xs = {}, s = {:?} }}",
            self.i,
            option(&self.o),
            list(&self.xs),
            self.s
        )
    }
}

fn main() {
    println!("{:?}", WithInner { i: Inner { v: 1.0 } });
    println!("{:?}", WithOption { o: Some(1.0) });
    println!("{:?}", WithOption { o: None });
    println!("{:?}", WithList { xs: vec![1.0, 2.0] });
    println!("{:?}", WithList { xs: vec![] });
    println!("{:?}", WithShape { s: Shape::Boxed(Inner { v: 2.0 }) });
    println!("{:?}", WithShape { s: Shape::Dot });
    println!("{:?}", Shape::Boxed(Inner { v: 2.0 }));
    let all = All { i: Inner { v: 1.0 }, o: Some(3.0), xs: vec![4.0], s: Shape::Dot };
    println!("{:?}", all);
}
