type Count = i64;

fn double(c: Count) -> Count {
    c + c
}

fn main() {
    println!("{}", double(7));
    println!("{}", double(21));
}
