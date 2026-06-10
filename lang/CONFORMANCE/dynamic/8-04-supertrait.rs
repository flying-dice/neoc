trait Show {
    fn show(self) -> String;
}

trait Greet: Show {
    fn greet(self) -> String;
}

struct Badger {
    name: String,
}

impl Show for Badger {
    fn show(self) -> String {
        self.name
    }
}

impl Greet for Badger {
    fn greet(self) -> String {
        "Hi, ".to_string() + &self.show()
    }
}

fn main() {
    let b = Badger {
        name: "Bram".to_string(),
    };
    println!("{}", b.greet());
}
