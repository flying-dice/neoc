trait Show {
    fn show(&self) -> String;
}

trait Base<T: Clone> {
    fn next(&self) -> T;

    fn peek(&self) -> T {
        self.next()
    }

    fn make(x: T) -> T {
        x
    }
}

trait Sub<T: Clone>: Base<T> {
    fn both(&self) -> Vec<T> {
        vec![self.peek(), self.next()]
    }
}

trait Pick<T> {
    fn get(&self) -> T;

    fn pick(&self) -> T {
        self.get()
    }
}

struct Namer {
    s: String,
}

struct Loud {
    s: String,
}

struct Quiet;

impl Base<String> for Namer {
    fn next(&self) -> String {
        self.s.clone()
    }
}

impl Sub<String> for Namer {}

impl Base<String> for Loud {
    fn next(&self) -> String {
        self.s.clone()
    }

    fn peek(&self) -> String {
        "loud".to_string()
    }
}

impl Show for Quiet {
    fn show(&self) -> String {
        "quiet".to_string()
    }
}

impl<S: Show> Pick<String> for S {
    fn get(&self) -> String {
        self.show()
    }
}

fn main() {
    let n = Namer { s: "n".to_string() };
    println!("{}", n.peek());
    println!("{}", n.both()[0]);
    println!("{}", <Namer as Base<String>>::make("m".to_string()));
    let l = Loud { s: "l".to_string() };
    println!("{}", l.peek());
    println!("{}", l.next());
    println!("{}", <Loud as Base<String>>::make("k".to_string()));
    println!("{}", Quiet.pick());
}
