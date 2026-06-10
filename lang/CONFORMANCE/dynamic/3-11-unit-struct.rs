struct Sun;

struct Moon;

impl Sun {
    fn shine(self) -> &'static str {
        "day"
    }
}

impl Moon {
    fn shine(self) -> &'static str {
        "night"
    }
}

fn main() {
    let a = Sun;
    let b = Moon;
    println!("{}", a.shine());
    println!("{}", b.shine());
}
