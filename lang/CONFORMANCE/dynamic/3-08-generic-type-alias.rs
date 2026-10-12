type Pair<T> = Vec<T>;
type Grid<T> = Pair<Pair<T>>;

struct Table {
    rows: Grid<f64>,
}

fn first<T: Clone>(items: &Pair<T>) -> T {
    items[0].clone()
}

fn main() {
    let grid: Grid<f64> = vec![vec![1.0, 2.0], vec![3.0, 4.0]];
    let table = Table { rows: grid };
    println!("{}", first(&first(&table.rows)));
    println!("{}", first(&table.rows[1]));
}
