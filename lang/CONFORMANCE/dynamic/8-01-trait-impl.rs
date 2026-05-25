trait Greet {
    fn greet(self) -> String;
}

struct Dog {
    name: String,
}

impl Greet for Dog {
    fn greet(self) -> String {
        self.name + " says woof"
    }
}

fn main() {
    let d = Dog {
        name: "Rex".to_string(),
    };
    println!("{}", d.greet());
}
