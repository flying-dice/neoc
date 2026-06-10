// neoc models `number` as `f64`; Rust derives `Clone` and (floats being only
// `PartialEq`) `PartialEq` for the `==`/`!=` this case exercises. neoc's derived
// Debug uses `=`-style decoration and its Display is decoration-free — neither
// matches Rust's `derive`d formats byte-for-byte, so both are hand-written to
// emit neoc's bytes: `Pulse(7)` / the bare `Idle` for Debug, `7` / `Idle` for
// Display.
use std::fmt;

#[derive(Clone, PartialEq)]
enum Signal {
    Pulse(f64),
    Idle,
}

impl fmt::Debug for Signal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Signal::Pulse(v) => write!(f, "Pulse({})", v),
            Signal::Idle => write!(f, "Idle"),
        }
    }
}

impl fmt::Display for Signal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Signal::Pulse(v) => write!(f, "{}", v),
            Signal::Idle => write!(f, "Idle"),
        }
    }
}

fn main() {
    let a = Signal::Pulse(7.0);
    let b = a.clone();
    if a == b {
        println!("clone-eq");
    }
    let c = Signal::Idle;
    if a != c {
        println!("tag-ne");
    }
    let d = Signal::Pulse(9.0);
    if a != d {
        println!("payload-ne");
    }
    println!("{:?}", a);
    println!("{:?}", c);
    println!("{}", a);
    println!("{}", c);
}
