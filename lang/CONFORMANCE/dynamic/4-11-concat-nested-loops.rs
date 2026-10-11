fn main() {
    let mut grid = String::new();
    for _r in 0..3 {
        for _c in 0..3 {
            grid += "#";
        }
        grid += "/";
    }
    println!("{}", grid);

    let mut log = String::new();
    for _r in 0..2 {
        for _c in 0..2 {
            log += "+";
        }
        println!("{}", log);
    }
}
