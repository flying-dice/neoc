struct Meters {
    value: i64,
}

impl Meters {
    fn origin() -> Self {
        Meters { value: 0 }
    }

    fn plus(self, other: Self) -> Self {
        Meters {
            value: self.value + other.value,
        }
    }
}

fn main() {
    let a = Meters { value: 3 };
    let b = Meters { value: 4 };
    println!("{}", a.plus(b).value);
    println!("{}", Meters::origin().value);
}
