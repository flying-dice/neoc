enum Dir {
    N,
    S,
    E,
    W,
}

impl Dir {
    fn name(self) -> &'static str {
        match self {
            Dir::N => "north",
            Dir::S => "south",
            Dir::E => "east",
            Dir::W => "west",
        }
    }
}

fn main() {
    let d = Dir::E;
    println!("{}", d.name());
}
