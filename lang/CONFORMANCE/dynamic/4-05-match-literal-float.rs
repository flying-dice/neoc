#[allow(illegal_floating_point_literal_pattern)]
fn name(x: f64) -> &'static str {
    match x {
        v if v == 0.5 => "half",
        v if v == 2.25 => "two-and-a-quarter",
        _ => "other",
    }
}

fn main() {
    println!("{}", name(0.5));
    println!("{}", name(2.25));
    println!("{}", name(3.0));
}
