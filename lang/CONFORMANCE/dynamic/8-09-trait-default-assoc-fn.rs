struct D;

impl Make for D {}

trait Make {
    fn tag() -> String {
        "made".to_string()
    }
}

fn main() {
    println!("{}", D::tag());
}
