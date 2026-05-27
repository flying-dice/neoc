trait Show {
    fn show(&self) -> String;
}

trait Describe {
    fn describe(&self) -> String;
}

impl<T: Show> Describe for T {
    fn describe(&self) -> String {
        self.show()
    }
}

struct Dog {
    name: String,
}

impl Show for Dog {
    fn show(&self) -> String {
        self.name.clone()
    }
}

fn main() {
    let d = Dog {
        name: "Rex".to_string(),
    };
    println!("{}", d.describe());
}
