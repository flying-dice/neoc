trait Source<T> {
    fn next(&self) -> T;
    fn feed(&self, item: T) -> f64;
}

struct Counter {
    n: f64,
}

impl Source<f64> for Counter {
    fn next(&self) -> f64 {
        self.n
    }

    fn feed(&self, item: f64) -> f64 {
        self.n + item
    }
}

fn pull(source: &dyn Source<f64>) -> f64 {
    source.next() + source.feed(10.0)
}

fn main() {
    let c = Counter { n: 4.0 };
    let a: f64 = c.next();
    println!("{}", a);
    println!("{}", c.feed(1.0));
    println!("{}", pull(&c));
}
