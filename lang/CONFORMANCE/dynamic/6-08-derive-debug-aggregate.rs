// neoc's derived Debug uses `Name { f = v }` decoration and renders `number`
// without a trailing `.0`; neither matches Rust's `derive`d format, so every
// `Debug` here is hand-written to emit neoc's bytes. `Option` and `Vec` render
// through small helpers in neoc's `Some(1)` / `None` / `[1, 2]` shape. The
// later cases render through `Neoc`, one impl per shape, so nesting composes;
// a string renders exactly as Rust's `{:?}` does.
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

trait Neoc {
    fn neoc(&self) -> String;
}

impl Neoc for f64 {
    fn neoc(&self) -> String {
        self.to_string()
    }
}

impl Neoc for String {
    fn neoc(&self) -> String {
        format!("{:?}", self)
    }
}

impl Neoc for Inner {
    fn neoc(&self) -> String {
        format!("{:?}", self)
    }
}

impl<T: Neoc> Neoc for Option<T> {
    fn neoc(&self) -> String {
        match self {
            Some(v) => format!("Some({})", v.neoc()),
            None => "None".to_string(),
        }
    }
}

impl<T: Neoc, E: Neoc> Neoc for Result<T, E> {
    fn neoc(&self) -> String {
        match self {
            Ok(v) => format!("Ok({})", v.neoc()),
            Err(e) => format!("Err({})", e.neoc()),
        }
    }
}

impl<T: Neoc> Neoc for Vec<T> {
    fn neoc(&self) -> String {
        let parts: Vec<String> = self.iter().map(Neoc::neoc).collect();
        format!("[{}]", parts.join(", "))
    }
}

struct Many {
    ys: Vec<Inner>,
    r: Result<f64, String>,
    oo: Option<Option<Inner>>,
}

impl fmt::Debug for Many {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Many {{ ys = {}, r = {}, oo = {} }}",
            self.ys.neoc(),
            self.r.neoc(),
            self.oo.neoc()
        )
    }
}

struct Wrap<T> {
    v: T,
}

impl<T: Neoc> fmt::Debug for Wrap<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Wrap {{ v = {} }}", self.v.neoc())
    }
}

// No `debug`: neoc elides the fields of a type without one.
#[allow(dead_code)]
struct Plain {
    v: f64,
}

// No `debug` either: neoc renders a tuple payload positionally, elides a named
// one, and prints a unit variant bare.
#[allow(dead_code)]
enum NoDbg {
    P(f64),
    N { v: f64 },
    U,
}

impl Neoc for NoDbg {
    fn neoc(&self) -> String {
        match self {
            NoDbg::P(v) => format!("P({})", v.neoc()),
            NoDbg::N { .. } => "N { .. }".to_string(),
            NoDbg::U => "U".to_string(),
        }
    }
}

impl Neoc for Plain {
    fn neoc(&self) -> String {
        "Plain { .. }".to_string()
    }
}

struct Text {
    s: String,
    o: Option<String>,
    xs: Vec<String>,
}

impl fmt::Debug for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Text {{ s = {}, o = {}, xs = {} }}",
            self.s.neoc(),
            self.o.neoc(),
            self.xs.neoc()
        )
    }
}

fn s(v: &str) -> String {
    v.to_string()
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
    println!("{:?}", Many { ys: vec![Inner { v: 1.0 }], r: Err(s("bad")), oo: Some(None) });
    println!("{:?}", Many { ys: vec![], r: Ok(2.0), oo: Some(Some(Inner { v: 3.0 })) });
    println!("{:?}", Wrap { v: Inner { v: 1.0 } });
    println!("{:?}", Wrap { v: 5.0 });
    println!("{:?}", Wrap { v: Plain { v: 1.0 } });
    println!("{:?}", Wrap { v: NoDbg::P(2.0) });
    println!("{:?}", Wrap { v: NoDbg::N { v: 3.0 } });
    println!("{:?}", Wrap { v: NoDbg::U });
    println!("{:?}", Text { s: s("a"), o: Some(s("b")), xs: vec![s("b"), s("c")] });
    println!("{:?}", Text { s: s("q\"x\\y"), o: None, xs: vec![s("b, c")] });
    println!("{:?}", Text { s: s("t\tn\nz"), o: None, xs: vec![] });
}
