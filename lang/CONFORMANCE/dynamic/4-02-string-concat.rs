fn greet(name: &str) -> String {
    "hello, ".to_string() + name + "!"
}

fn main() {
    println!("{}", greet("world"));
}
