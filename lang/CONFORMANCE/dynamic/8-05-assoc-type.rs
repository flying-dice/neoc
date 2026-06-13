trait Container {
    type Item;
    fn peek(self) -> Self::Item;
}

struct Bin {
    value: f64,
}

impl Container for Bin {
    type Item = f64;
    fn peek(self) -> Self::Item {
        self.value
    }
}

fn main() {
    let b = Bin { value: 7.0 };
    println!("{}", b.peek());
}
