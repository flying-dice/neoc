trait Doubler {
    fn double(self) -> f64;
}

struct Num {
    v: f64,
}

impl Doubler for Num {
    fn double(self) -> f64 {
        self.v + self.v
    }
}

fn combine<A, B>(a: A, b: B) -> f64
where
    A: Doubler,
    B: Doubler,
{
    a.double() + b.double()
}

fn main() {
    println!("{}", combine(Num { v: 21.0 }, Num { v: 5.0 }));
}
