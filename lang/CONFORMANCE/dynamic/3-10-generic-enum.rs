enum Tree<T> {
    Leaf(T),
    Empty,
}

fn value(t: Tree<f64>) -> f64 {
    match t {
        Tree::Leaf(n) => n,
        Tree::Empty => 0.0,
    }
}

fn main() {
    println!("{}", value(Tree::Leaf(5.0)));
    println!("{}", value(Tree::Empty));
}
