use super::*;
use crate::ui::node::Action;
use serde_json::json;

fn leaf(key: &str) -> Node {
    Node::new("Text", key)
}

fn list(keys: &[&str]) -> Node {
    keys.iter().fold(Node::new("Stack", "main"), |stack, key| stack.child(leaf(key)))
}

fn roundtrip(old: &Node, new: &Node) -> Vec<Op> {
    let ops = diff(old, new);
    let mut applied = old.clone();
    apply(&mut applied, &ops).unwrap_or_else(|e| panic!("{e}: {ops:?}"));
    assert_eq!(&applied, new, "ops: {ops:?}");
    ops
}

fn count(ops: &[Op], pick: fn(&Op) -> bool) -> usize {
    ops.iter().filter(|op| pick(op)).count()
}

#[test]
fn equal_trees_have_no_ops() {
    let tree = list(&["a", "b"]).prop("spacing", json!(2));
    assert!(diff(&tree, &tree.clone()).is_empty());
}

#[test]
fn a_changed_prop_is_one_set_with_only_that_prop() {
    let old = Node::new("Stack", "main")
        .child(Node::new("Text", "display").prop("text", json!("0")).prop("style", json!("body")));
    let new = Node::new("Stack", "main")
        .child(Node::new("Text", "display").prop("text", json!("7")).prop("style", json!("body")));
    let ops = roundtrip(&old, &new);
    assert_eq!(
        ops,
        vec![Op::Set { path: "main/display".into(), props: [("text".to_owned(), json!("7"))].into() }]
    );
}

#[test]
fn a_removed_prop_is_set_to_null() {
    let old = Node::new("Text", "main").prop("text", json!("a")).prop("lines", json!(1));
    let new = Node::new("Text", "main").prop("text", json!("a"));
    let ops = roundtrip(&old, &new);
    assert_eq!(ops, vec![Op::Set { path: "main".into(), props: [("lines".to_owned(), Value::Null)].into() }]);
}

#[test]
fn handler_changes_are_on_ops() {
    let old = Node::new("Button", "main").handler("tap", Handler::Emit).handler("longPress", Handler::Emit);
    let new = Node::new("Button", "main").handler("tap", Handler::Actions(vec![Action::NavPop]));
    let ops = roundtrip(&old, &new);
    assert_eq!(ops.len(), 1);
    let Op::On { on, .. } = &ops[0] else { panic!("expected an on op, got {ops:?}") };
    assert_eq!(on.get("longPress"), Some(&None));
    assert!(matches!(on.get("tap"), Some(Some(Handler::Actions(_)))));
}

#[test]
fn append_is_one_insert() {
    let ops = roundtrip(&list(&["a", "b"]), &list(&["a", "b", "c"]));
    assert_eq!(ops.len(), 1);
    assert!(matches!(&ops[0], Op::Insert { before: None, slot: None, node, .. } if node.key == "c"));
}

#[test]
fn insert_in_the_middle_names_the_next_sibling() {
    let ops = roundtrip(&list(&["a", "c"]), &list(&["a", "b", "c"]));
    assert_eq!(ops.len(), 1);
    assert!(matches!(&ops[0], Op::Insert { before: Some(k), .. } if k == "c"));
}

#[test]
fn remove_is_one_remove_by_key_path() {
    let ops = roundtrip(&list(&["a", "b", "c"]), &list(&["a", "c"]));
    assert_eq!(ops, vec![Op::Remove { path: "main/b".into() }]);
}

#[test]
fn moving_one_item_is_one_move() {
    let ops = roundtrip(&list(&["a", "b", "c", "d", "e"]), &list(&["a", "c", "d", "e", "b"]));
    assert_eq!(ops, vec![Op::Move { path: "main/b".into(), before: None }]);
    let ops = roundtrip(&list(&["a", "b", "c", "d", "e"]), &list(&["e", "a", "b", "c", "d"]));
    assert_eq!(ops, vec![Op::Move { path: "main/e".into(), before: Some("a".into()) }]);
}

#[test]
fn reversing_n_items_takes_n_minus_one_moves() {
    let ops = roundtrip(&list(&["a", "b", "c", "d", "e"]), &list(&["e", "d", "c", "b", "a"]));
    assert_eq!(count(&ops, |o| matches!(o, Op::Move { .. })), 4);
    assert_eq!(ops.len(), 4);
}

#[test]
fn a_type_change_is_a_replace_not_a_recursion() {
    let old = Node::new("Stack", "main").child(Node::new("Text", "x").prop("text", json!("hi")));
    let new = Node::new("Stack", "main").child(Node::new("Label", "x").prop("title", json!("hi")));
    let ops = roundtrip(&old, &new);
    assert_eq!(ops.len(), 1);
    assert!(matches!(&ops[0], Op::Replace { path, node } if path == "main/x" && node.type_ == "Label"));
}

#[test]
fn a_root_type_change_replaces_the_root() {
    let ops = roundtrip(&Node::new("Stack", "main"), &Node::new("Group", "main"));
    assert!(matches!(&ops[..], [Op::Replace { path, .. }] if path == "main"));
}

#[test]
fn slots_are_addressed_with_at_segments() {
    let row = |text: &str| Node::new("Row", "_0").child(Node::new("Text", "t").prop("text", json!(text)));
    let old = Node::new("List", "main").slot("row", vec![row("a")]);
    let new = Node::new("List", "main").slot("row", vec![row("b")]);
    let ops = roundtrip(&old, &new);
    assert_eq!(ops.len(), 1);
    assert!(matches!(&ops[0], Op::Set { path, .. } if path == "main/@row/_0/t"));
}

#[test]
fn slots_appear_and_disappear() {
    let old = Node::new("Show", "main");
    let new = Node::new("Show", "main").slot("otherwise", vec![leaf("x")]);
    let ops = roundtrip(&old, &new);
    assert!(matches!(&ops[..], [Op::Insert { slot: Some(s), .. }] if s == "otherwise"));
    roundtrip(&new, &old);
}

#[test]
fn ops_serialize_in_the_spec_shape() {
    let op = Op::Insert { parent: "main".into(), slot: None, before: None, node: leaf("a") };
    let json = serde_json::to_value(&op).unwrap();
    assert_eq!(json["op"], "insert");
    assert_eq!(json["before"], Value::Null, "`before: null` appends and is always present");
    assert!(json.get("slot").is_none());
    assert_eq!(serde_json::from_value::<Op>(json).unwrap(), op);
}

#[test]
fn apply_reports_a_missing_node_so_the_client_can_resync() {
    let mut tree = list(&["a"]);
    let err =
        apply(&mut tree, &[Op::Set { path: "main/nope".into(), props: [("x".to_owned(), json!(1))].into() }]);
    assert_eq!(err, Err(ApplyError::NoNodeAtPath("main/nope".into())));
    let err = apply(
        &mut tree,
        &[Op::Insert { parent: "main".into(), slot: None, before: Some("zzz".into()), node: leaf("n") }],
    );
    assert!(matches!(err, Err(ApplyError::NoSibling { .. })));
    let err =
        apply(&mut tree, &[Op::Insert { parent: "main".into(), slot: None, before: None, node: leaf("a") }]);
    assert!(matches!(err, Err(ApplyError::DuplicateKey { .. })));
}

#[test]
fn remove_is_idempotent() {
    let mut tree = list(&["a"]);
    let remove = Op::Remove { path: "main/zzz".into() };
    assert!(apply(&mut tree, &[remove.clone(), remove]).is_ok());
    assert_eq!(tree, list(&["a"]));
}

#[test]
fn removing_the_last_node_of_a_slot_removes_the_slot() {
    let mut tree = Node::new("Show", "main").slot("otherwise", vec![leaf("x")]);
    apply(&mut tree, &[Op::Remove { path: "main/@otherwise/x".into() }]).unwrap();
    assert_eq!(tree, Node::new("Show", "main"));
}

#[test]
fn the_lis_is_a_longest_increasing_subsequence() {
    let lis = |v: &[usize]| longest_increasing_subsequence(v).into_iter().map(|i| v[i]).collect::<Vec<_>>();
    assert_eq!(lis(&[]), Vec::<usize>::new());
    assert_eq!(lis(&[0, 1, 2, 3]), [0, 1, 2, 3]);
    assert_eq!(lis(&[3, 2, 1, 0]).len(), 1);
    assert_eq!(lis(&[2, 0, 1, 3, 4]).len(), 4);
    let mixed = lis(&[5, 1, 6, 2, 7, 3, 8]);
    assert_eq!(mixed.len(), 4);
    assert!(mixed.windows(2).all(|w| w[0] < w[1]));
}

// ---- property-style tests: apply(diff(a, b), a) == b over generated trees --------------------------------

/// xorshift64*: deterministic, no dependencies.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % n as u64).unwrap_or(0)
    }
    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
}

const KEYS: [&str; 7] = ["a", "b", "c", "d", "e", "f", "_0"];
const TYPES: [&str; 3] = ["Stack", "Text", "Group"];

fn random_value(rng: &mut Rng) -> Value {
    match rng.below(5) {
        0 => json!(1),
        1 => json!("x"),
        2 => json!(true),
        3 => json!({ "$ref": "client.q" }),
        _ => json!({ "k": [1, 2] }),
    }
}

fn random_keys(rng: &mut Rng, max: usize) -> Vec<&'static str> {
    let mut pool: Vec<&'static str> = KEYS.to_vec();
    let mut keys = Vec::new();
    for _ in 0..rng.below(max + 1) {
        keys.push(pool.remove(rng.below(pool.len())));
    }
    keys
}

fn random_node(rng: &mut Rng, key: &str, depth: usize) -> Node {
    let mut node = Node::new(TYPES[rng.below(TYPES.len())], key);
    for name in ["p0", "p1", "p2"] {
        if rng.chance(40) {
            node.props.insert(name.to_owned(), random_value(rng));
        }
    }
    for event in ["tap", "change"] {
        if rng.chance(20) {
            let handler = if rng.chance(50) { Handler::Emit } else { Handler::Actions(vec![Action::NavPop]) };
            node.on.insert(event.to_owned(), handler);
        }
    }
    if depth > 0 {
        for key in random_keys(rng, 5) {
            node.children.push(random_node(rng, key, depth - 1));
        }
        for slot in ["row", "aux"] {
            let keys = random_keys(rng, 3);
            if !keys.is_empty() {
                node.slots.insert(
                    slot.to_owned(),
                    keys.into_iter().map(|k| random_node(rng, k, depth - 1)).collect(),
                );
            }
        }
    }
    node
}

/// A tree close to `tree`: the interesting case, where most keys survive and move around.
fn mutate(rng: &mut Rng, tree: &Node) -> Node {
    let mut next = tree.clone();
    mutate_node(rng, &mut next, 3);
    next
}

fn mutate_node(rng: &mut Rng, node: &mut Node, depth: usize) {
    if rng.chance(30) {
        node.props.insert("p0".to_owned(), random_value(rng));
    }
    if rng.chance(15) {
        node.props.remove("p1");
    }
    if rng.chance(10) {
        node.on.insert("tap".to_owned(), Handler::Emit);
    }
    mutate_list(rng, &mut node.children, depth);
    for list in node.slots.values_mut() {
        mutate_list(rng, list, depth);
    }
    node.slots.retain(|_, list| !list.is_empty());
    if rng.chance(5) {
        let fresh = random_node(rng, "x", 1);
        node.slots.entry("aux".to_owned()).or_default().retain(|n| n.key != "x");
        node.slots.entry("aux".to_owned()).or_default().push(fresh);
    }
}

fn mutate_list(rng: &mut Rng, list: &mut Vec<Node>, depth: usize) {
    if rng.chance(30) && list.len() > 1 {
        let (i, j) = (rng.below(list.len()), rng.below(list.len()));
        list.swap(i, j);
    }
    if rng.chance(25) && !list.is_empty() {
        let at = rng.below(list.len());
        let node = list.remove(at);
        let to = rng.below(list.len() + 1);
        list.insert(to, node);
    }
    if rng.chance(20) && !list.is_empty() {
        list.remove(rng.below(list.len()));
    }
    if rng.chance(25) {
        let taken: Vec<String> = list.iter().map(|n| n.key.clone()).collect();
        if let Some(key) = KEYS.iter().find(|k| !taken.iter().any(|t| t == *k)) {
            let at = rng.below(list.len() + 1);
            list.insert(at, random_node(rng, key, 1));
        }
    }
    if rng.chance(10) && !list.is_empty() {
        let at = rng.below(list.len());
        list[at].type_ = TYPES[rng.below(TYPES.len())].to_owned();
    }
    if depth > 0 {
        for node in list.iter_mut() {
            if rng.chance(50) {
                mutate_node(rng, node, depth - 1);
            }
        }
    }
}

/// The generated-tree properties run a fifth of their cases in the gate; `bun run slow` runs all of them (the
/// `#[ignore]`d `_full` tests below).
const GATE_CASES: usize = 300;

fn apply_reproduces_the_new_tree_for_unrelated_trees(cases: usize) {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..cases {
        let (a, b) = (random_node(&mut rng, "main", 3), random_node(&mut rng, "main", 3));
        let ops = diff(&a, &b);
        let mut applied = a.clone();
        apply(&mut applied, &ops).unwrap_or_else(|e| panic!("{e}\nold: {a:#?}\nnew: {b:#?}\nops: {ops:#?}"));
        assert_eq!(applied, b, "old: {a:#?}\nops: {ops:#?}");
    }
}

#[test]
fn apply_of_diff_reproduces_the_new_tree_for_unrelated_trees() {
    apply_reproduces_the_new_tree_for_unrelated_trees(GATE_CASES);
}

#[test]
#[ignore = "slow: 1500 generated trees in a debug build; run with `bun run slow` (scripts/check-slow.sh)"]
fn apply_of_diff_reproduces_the_new_tree_for_unrelated_trees_full() {
    apply_reproduces_the_new_tree_for_unrelated_trees(1500);
}

fn apply_reproduces_the_new_tree_for_edited_trees(cases: usize) {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    let mut changed = 0;
    for _ in 0..cases {
        let a = random_node(&mut rng, "main", 3);
        let b = mutate(&mut rng, &a);
        let ops = diff(&a, &b);
        changed += usize::from(!ops.is_empty());
        let mut applied = a.clone();
        apply(&mut applied, &ops).unwrap_or_else(|e| panic!("{e}\nold: {a:#?}\nnew: {b:#?}\nops: {ops:#?}"));
        assert_eq!(applied, b, "old: {a:#?}\nops: {ops:#?}");
    }
    assert!(changed > cases / 2, "the generator must actually change trees ({changed} of {cases})");
}

#[test]
fn apply_of_diff_reproduces_the_new_tree_for_edited_trees() {
    apply_reproduces_the_new_tree_for_edited_trees(GATE_CASES * 2);
}

#[test]
#[ignore = "slow: 3000 generated trees in a debug build; run with `bun run slow` (scripts/check-slow.sh)"]
fn apply_of_diff_reproduces_the_new_tree_for_edited_trees_full() {
    apply_reproduces_the_new_tree_for_edited_trees(3000);
}

#[test]
fn diffing_a_tree_with_itself_is_empty_over_generated_trees() {
    let mut rng = Rng(42);
    for _ in 0..300 {
        let tree = random_node(&mut rng, "main", 3);
        assert!(diff(&tree, &tree.clone()).is_empty());
    }
}

#[test]
fn a_sibling_reorder_never_emits_more_moves_than_items() {
    let mut rng = Rng(7);
    for _ in 0..500 {
        let keys = random_keys(&mut rng, 6);
        let mut shuffled = keys.clone();
        for i in (1..shuffled.len()).rev() {
            shuffled.swap(i, rng.below(i + 1));
        }
        let ops = roundtrip(&list(&keys), &list(&shuffled));
        assert!(ops.len() < keys.len().max(1), "{keys:?} -> {shuffled:?}: {ops:?}");
    }
}
