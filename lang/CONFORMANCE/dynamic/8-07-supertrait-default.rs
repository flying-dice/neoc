trait Show {
    fn show(&self) -> String;

    fn shout(&self) -> String {
        self.show() + "!"
    }
}

trait Greet: Show {
    fn greet(&self) -> String {
        "Hi, ".to_string() + &self.shout()
    }
}

struct Badger {
    name: String,
}

impl Show for Badger {
    fn show(&self) -> String {
        self.name.clone()
    }
}

impl Greet for Badger {}

fn main() {
    let b = Badger { name: "Bram".to_string() };
    println!("{}", b.shout());
    println!("{}", b.greet());
}
