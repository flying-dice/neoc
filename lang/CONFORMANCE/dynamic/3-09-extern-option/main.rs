mod host {
    use std::collections::HashMap;

    pub struct Counter(f64);

    impl Counter {
        pub fn new(initial: Option<f64>) -> Counter {
            Counter(initial.unwrap_or(0.0))
        }

        pub fn get(&self) -> f64 {
            self.0
        }
    }

    pub struct Map<V>(HashMap<String, V>);

    impl<V: Copy> Map<V> {
        pub fn new() -> Map<V> {
            Map(HashMap::new())
        }

        pub fn set(&mut self, key: &str, value: V) {
            self.0.insert(key.to_string(), value);
        }

        pub fn get(&self, key: &str) -> Option<V> {
            self.0.get(key).copied()
        }
    }
}

use host::{Counter, Map};

fn main() {
    let seeded = Counter::new(Some(5.0));
    let bare = Counter::new(None);
    println!("{}", seeded.get());
    println!("{}", bare.get());

    let mut m: Map<&str> = Map::new();
    m.set("a", "x");
    println!("{}", m.get("a").unwrap_or("none"));
    println!("{}", m.get("z").unwrap_or("none"));
    println!("{}", m.get("z").is_none());

    let mut flags: Map<bool> = Map::new();
    flags.set("off", false);
    println!("{}", flags.get("off").is_some());
    println!("{}", flags.get("off").unwrap_or(true));
}
