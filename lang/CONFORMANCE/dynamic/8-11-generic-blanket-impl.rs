trait Named {
    fn name(&self) -> String;
}

trait Source<T> {
    fn next(&self) -> T;
}

impl<U: Named> Source<f64> for U {
    fn next(&self) -> f64 {
        if self.name() == "a" { 7.0 } else { 0.0 }
    }
}

struct A {
    n: String,
}

impl Named for A {
    fn name(&self) -> String {
        self.n.clone()
    }
}

fn pull(source: &dyn Source<f64>) -> f64 {
    source.next() + 1.0
}

fn main() {
    let a = A { n: "a".to_string() };
    println!("{}", a.next());
    println!("{}", pull(&a));
}
