struct Foo {
    a: f64,
}

type Pt = Foo;
#[allow(dead_code)]
type Green = Foo;

enum Color {
    Red,
    Green(f64),
    Pt { a: f64 },
}

fn pick(c: Color) -> f64 {
    match c {
        Color::Red => 0.0,
        Color::Green(n) => n,
        Color::Pt { a } => a,
    }
}

struct Pair<A, B> {
    #[allow(dead_code)]
    a: A,
    b: B,
}

type PB = Pair<bool, f64>;

trait Sup<X> {
    fn s(self) -> X;
}

impl Sup<f64> for PB {
    fn s(self) -> f64 {
        self.b
    }
}

fn through<T: Sup<f64>>(t: T) -> f64 {
    t.s()
}

fn main() {
    println!("{}", pick(Color::Red));
    println!("{}", pick(Color::Green(4.0)));
    println!("{}", pick(Color::Pt { a: 5.0 }));
    println!("{}", through(Pair { a: true, b: 6.0 }));
    println!("{}", Pt { a: 7.0 }.a);
}
