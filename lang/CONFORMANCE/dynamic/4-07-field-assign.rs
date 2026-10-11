use std::cell::RefCell;
use std::rc::Rc;

struct Cell {
    value: i64,
}

fn main() {
    let c = Rc::new(RefCell::new(Cell { value: 1 }));
    let alias = Rc::clone(&c);
    c.borrow_mut().value = 5;
    println!("{}", alias.borrow().value);
    let next = c.borrow().value + 2;
    alias.borrow_mut().value = next;
    println!("{}", c.borrow().value);
}
