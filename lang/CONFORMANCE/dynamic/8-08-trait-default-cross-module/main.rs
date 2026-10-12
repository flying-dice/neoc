mod greet {
    pub trait Greet {
        fn name(&self) -> String;

        fn hi(&self) -> String {
            prefix() + &self.name()
        }
    }

    fn prefix() -> String {
        "hi ".to_string()
    }
}

use greet::Greet;

struct D;

impl Greet for D {
    fn name(&self) -> String {
        "d".to_string()
    }
}

fn main() {
    let d = D;
    println!("{}", d.hi());
}
