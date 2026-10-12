#![allow(non_snake_case, non_upper_case_globals, dead_code)]

// Rust keeps types and values in separate namespaces, as neoc does.
#[derive(Clone)]
struct Hue {
    r: f64,
}

struct Pt {
    x: f64,
}

struct Box_ {
    v: f64,
}

enum Mode {
    On,
    Off,
}

trait Score {
    fn base(&self) -> f64;

    fn bonus(&self) -> f64 {
        self.base() + 100.0
    }
}

const Pt: f64 = 5.0;

fn Box() -> f64 {
    6.0
}

impl Pt {
    // Rust cannot bind a parameter named like the `const Pt`; the neoc case
    // does (`fn make(Pt: number)`), a difference of the host language only.
    fn make(x: f64) -> Pt {
        Pt { x }
    }
}

impl Mode {
    fn code(&self) -> f64 {
        match self {
            Mode::On => 1.0,
            Mode::Off => 0.0,
        }
    }
}

impl Score for Pt {
    fn base(&self) -> f64 {
        self.x
    }
}

fn pick(Mode: f64) -> f64 {
    let m = Mode::On;
    Mode + m.code()
}

fn main() {
    let Hue = 7.0;
    let h = Hue { r: Hue };
    println!("{}", h.r);
    let grow = |Hue: f64| Hue { r: Hue + 1.0 };
    println!("{}", grow(1.0).r);
    println!("{}", h.clone().r);
    println!("{}", Pt);
    println!("{}", Pt::make(3.0).x);
    println!("{}", Pt::make(3.0).bonus());
    println!("{}", Box());
    println!("{}", (Box_ { v: Box() }).v);
    println!("{}", pick(10.0));
    for Hue in 0..2 {
        println!("{}", Hue);
    }
    println!("{}", Pt::make(2.0).x);
}
