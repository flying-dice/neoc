trait Greet {
    fn name(&self) -> String;

    fn hi(&self) -> String {
        "hi ".to_string() + &self.name()
    }
}

struct Quiet {
    n: String,
}

struct Loud {
    n: String,
}

impl Greet for Quiet {
    fn name(&self) -> String {
        self.n.clone()
    }
}

impl Greet for Loud {
    fn name(&self) -> String {
        self.n.clone()
    }

    fn hi(&self) -> String {
        "HEY ".to_string() + &self.name()
    }
}

fn main() {
    let q = Quiet { n: "ann".to_string() };
    let l = Loud { n: "bob".to_string() };
    println!("{}", q.hi());
    println!("{}", l.hi());
}
