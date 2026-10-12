trait Source<T: Clone> {
    fn next(&self) -> T;

    fn peek(&self) -> T {
        self.next()
    }

    fn twice(&self, item: T) -> Vec<T> {
        vec![item, self.peek()]
    }
}

struct Counter {
    n: f64,
}

struct Namer {
    s: String,
}

impl Source<f64> for Counter {
    fn next(&self) -> f64 {
        self.n
    }
}

impl Source<String> for Namer {
    fn next(&self) -> String {
        self.s.clone()
    }
}

struct Box<U> {
    v: U,
}

impl<U: Clone> Source<U> for Box<U> {
    fn next(&self) -> U {
        self.v.clone()
    }
}

fn pull<T: Clone>(s: &dyn Source<T>) -> Vec<T> {
    s.twice(s.peek())
}

fn main() {
    let c = Counter { n: 4.0 };
    println!("{}", c.peek());
    println!("{}", c.twice(9.0)[0]);
    println!("{}", c.twice(9.0)[1]);
    let n = Namer { s: "a".to_string() };
    let _t: String = n.peek();
    let b = Box { v: 7.0 };
    println!("{}", b.peek());
    println!("{}", pull(&c)[0]);
    println!("{}", pull(&b)[1]);
}
