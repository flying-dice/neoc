mod util {
    pub fn add(a: f64, b: f64) -> f64 {
        a + b
    }

    pub struct Counter {
        pub n: f64,
    }

    impl Counter {
        pub fn bump(&self) -> f64 {
            self.n + 1.0
        }
    }
}

mod geo {
    pub mod shape {
        pub enum Kind {
            Small,
            Large,
        }

        pub struct Circle {
            pub r: f64,
        }

        impl Circle {
            pub fn area(&self) -> f64 {
                3.0 * self.r * self.r
            }

            pub fn kind(&self) -> Kind {
                if self.r > 2.0 { Kind::Large } else { Kind::Small }
            }
        }
    }
}

use geo::shape::{Circle, Kind};
use util::add as plus;
use util::Counter;

fn main() {
    println!("{}", plus(2.0, 3.0));
    let c = Counter { n: 4.0 };
    println!("{}", c.bump());
    let circle = Circle { r: 3.0 };
    println!("{}", circle.area());
    let rank = match circle.kind() {
        Kind::Small => 1,
        Kind::Large => 2,
    };
    println!("{}", rank);
}
