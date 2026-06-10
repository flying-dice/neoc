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

fn twice<T: Doubler>(x: T) -> f64 {
    x.double()
}

fn main() {
    println!("{}", twice(Num { v: 21.0 }));
}
