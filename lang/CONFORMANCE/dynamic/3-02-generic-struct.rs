// Named `Holder` rather than `Box` so the anchor does not shadow the std
// prelude's `Box`; the neoc case names it `Box`, but only the printed bytes are
// pinned across the two runners, not the type name.
struct Holder<T> {
    value: T,
}

impl<T> Holder<T> {
    fn get(self) -> T {
        self.value
    }
}

fn main() {
    let b = Holder { value: 99.0 };
    println!("{}", b.get());
}
