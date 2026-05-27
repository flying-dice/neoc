struct Counter {
    value: f64,
}

impl Counter {
    fn start(v: f64) -> Counter {
        Counter { value: v }
    }

    fn bump(self) -> f64 {
        self.value + 1.0
    }
}

fn main() {
    let c = Counter::start(41.0);
    println!("{}", c.value);
    println!("{}", c.bump());
}
