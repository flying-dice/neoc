use std::cell::RefCell;

fn main() {
    let s = RefCell::new(String::new());
    let peek = || s.borrow().clone();
    for _i in 0..3 {
        s.borrow_mut().push_str("z");
        println!("{}", peek());
    }
}
