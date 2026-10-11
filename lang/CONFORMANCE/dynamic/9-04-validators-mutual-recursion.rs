struct User {
    name: String,
    addresses: Vec<Address>,
}

struct Address {
    zip: String,
    resident: Option<Box<User>>,
}

fn main() {
    let home = Address {
        zip: "10001".to_string(),
        resident: None,
    };
    let ada = User {
        name: "ada".to_string(),
        addresses: vec![home],
    };
    let work = Address {
        zip: "94105".to_string(),
        resident: Some(Box::new(ada)),
    };
    let grace = User {
        name: "grace".to_string(),
        addresses: vec![work],
    };
    match &grace.addresses[0].resident {
        Some(u) => println!("{}", u.name),
        None => println!("none"),
    }
    println!("{}", grace.addresses[0].zip);
}
