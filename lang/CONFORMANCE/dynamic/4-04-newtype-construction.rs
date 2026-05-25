#[derive(Clone, Copy)]
struct Wrapper(f64);

impl Wrapper {
    fn get(self) -> f64 {
        let Wrapper(n) = self;
        n
    }
}

fn main() {
    let w = Wrapper(42.0);
    println!("{}", w.get());
    let Wrapper(m) = w;
    println!("{}", m);
}
