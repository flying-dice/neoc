mod host {
    pub fn floor(x: f64) -> f64 {
        x.floor()
    }

    pub fn encode(input: &str) -> String {
        input.bytes().map(|b| format!("{b:02x}")).collect()
    }

    use std::collections::HashMap;

    pub struct Map(HashMap<String, f64>);

    impl Map {
        pub fn new() -> Map {
            Map(HashMap::new())
        }

        pub fn set(&mut self, key: &str, value: f64) {
            self.0.insert(key.to_string(), value);
        }

        pub fn has(&self, key: &str) -> bool {
            self.0.contains_key(key)
        }

        pub fn size(&self) -> f64 {
            self.0.len() as f64
        }
    }
}

use host::{encode, floor, Map};

fn main() {
    println!("{}", floor(7.9));
    println!("{}", encode("hi"));
    let mut m = Map::new();
    m.set("a", 1.0);
    m.set("b", 2.0);
    println!("{}", m.has("a"));
    println!("{}", m.has("z"));
    println!("{}", m.size());
}
