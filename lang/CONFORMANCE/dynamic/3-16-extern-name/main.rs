mod host {
    use std::collections::HashMap;

    pub fn round_down(x: f64) -> f64 {
        x.floor()
    }

    pub struct Flags(HashMap<String, f64>);

    impl Flags {
        pub fn new() -> Flags {
            Flags(HashMap::new())
        }

        pub fn set(&mut self, key: &str, value: f64) {
            self.0.insert(key.to_string(), value);
        }

        pub fn size(&self) -> f64 {
            self.0.len() as f64
        }
    }
}

use host::{round_down, Flags};

fn say(v: f64) {
    println!("{}", v);
}

fn main() {
    say(round_down(7.9));
    let mut f = Flags::new();
    f.set("a", 1.0);
    f.set("b", 2.0);
    say(f.size());
}
