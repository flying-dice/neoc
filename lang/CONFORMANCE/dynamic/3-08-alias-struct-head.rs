struct P {
    x: f64,
}

type A = P;
type B = A;

impl A {
    fn mk() -> P {
        P { x: 7.0 }
    }

    fn make(x: f64) -> Self {
        Self { x }
    }

    fn doubled(self) -> f64 {
        self.x * 2.0
    }
}

trait Show {
    fn show(self) -> f64;
}

impl Show for B {
    fn show(self) -> f64 {
        self.x + 1.0
    }
}

struct W<T> {
    v: T,
}

impl N {
    fn three() -> W<f64> {
        Self { v: 3.0 }
    }
}

type N = W<f64>;

enum Color {
    Red,
    Green(f64),
    Pt { a: f64 },
}

type C = Color;

fn pick(c: C) -> f64 {
    match c {
        C::Red => 0.0,
        C::Green(n) => n,
        C::Pt { a } => a,
    }
}

fn unwrap(a: A) -> f64 {
    let A { x } = a;
    x
}

fn main() {
    let a = A { x: 4.0 };
    let b = B { x: 5.0 };
    println!("{}", a.x);
    println!("{}", b.doubled());
    println!("{}", A::mk().x);
    println!("{}", B::make(6.0).doubled());
    println!("{}", a.show());
    println!("{}", N::three().v);
    println!("{}", W { v: 8.0 }.v);
    println!("{}", pick(C::Green(9.0)));
    println!("{}", pick(C::Red));
    println!("{}", pick(C::Pt { a: 11.0 }));
    println!("{}", unwrap(A { x: 2.0 }));
}
