#![allow(unused)]
#[derive(Clone)]
struct Vec3 {
    x: f64,
    y: f64,
    z: f64,
}

fn main() {
    let v = Vec3 { x: 1.0, y: 2.0, z: 3.0 };
    let w = Vec3 { x: 10.0, z: 30.0, ..v };
    println!("{}", w.x);
    println!("{}", w.y);
    println!("{}", w.z);
}
