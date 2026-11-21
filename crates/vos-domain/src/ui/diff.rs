//! Keyed reconciliation: `diff(old, new)` is what the host runs after every render, and
//! [`apply`] is what a client (or a test) runs on the result.
//!
//! The host reconciles the new tree against the instance's previous tree React-style:
//!
//! * same `(type, key)`: recurse and diff props, handlers and children;
//! * sibling key sequences: removed keys are `remove`d, new keys are `insert`ed, and keys that changed
//!   relative order are `move`d, using a longest-increasing-subsequence pass so that the keys which can stay put
//!   do stay put (minimal moves);
//! * same key, new type: `replace`.
//!
//! Ops address nodes by **key path**, never by index, so a node keeps its client-side state (focus, scroll,
//! drafts) across reorders. An empty slot and an absent slot are the same thing.
//!
//! `diff` assumes both trees are valid ([`super::validate`]): with duplicate sibling keys the result is
//! unspecified (but never panics).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde_json::Value;

use super::node::{join_path, Handler, Node};
use super::protocol::Op;

/// The ops that turn `old` into `new`. Empty when the trees are equal.
#[must_use]
pub fn diff(old: &Node, new: &Node) -> Vec<Op> {
    let mut ops = Vec::new();
    if old.type_ != new.type_ || old.key != new.key {
        ops.push(Op::Replace { path: old.key.clone(), node: new.clone() });
    } else {
        diff_node(old, new, &old.key, &mut ops);
    }
    ops
}

fn diff_node(old: &Node, new: &Node, path: &str, ops: &mut Vec<Op>) {
    diff_props(old, new, path, ops);
    diff_handlers(old, new, path, ops);
    let slots: BTreeSet<&str> = old.slots.keys().chain(new.slots.keys()).map(String::as_str).collect();
    diff_children(&old.children, &new.children, path, None, ops);
    for slot in slots {
        let empty = Vec::new();
        let before = old.slots.get(slot).unwrap_or(&empty);
        let after = new.slots.get(slot).unwrap_or(&empty);
        diff_children(before, after, path, Some(slot), ops);
    }
}

fn diff_props(old: &Node, new: &Node, path: &str, ops: &mut Vec<Op>) {
    let mut changed: BTreeMap<String, Value> = BTreeMap::new();
    for (name, value) in &new.props {
        if old.props.get(name) != Some(value) {
            changed.insert(name.clone(), value.clone());
        }
    }
    for name in old.props.keys().filter(|name| !new.props.contains_key(*name)) {
        changed.insert(name.clone(), Value::Null);
    }
    if !changed.is_empty() {
        ops.push(Op::Set { path: path.to_owned(), props: changed });
    }
}

fn diff_handlers(old: &Node, new: &Node, path: &str, ops: &mut Vec<Op>) {
    let mut changed: BTreeMap<String, Option<Handler>> = BTreeMap::new();
    for (event, handler) in &new.on {
        if old.on.get(event) != Some(handler) {
            changed.insert(event.clone(), Some(handler.clone()));
        }
    }
    for event in old.on.keys().filter(|event| !new.on.contains_key(*event)) {
        changed.insert(event.clone(), None);
    }
    if !changed.is_empty() {
        ops.push(Op::On { path: path.to_owned(), on: changed });
    }
}

fn diff_children(old: &[Node], new: &[Node], parent: &str, slot: Option<&str>, ops: &mut Vec<Op>) {
    let old_index: HashMap<&str, usize> = old.iter().enumerate().map(|(i, n)| (n.key.as_str(), i)).collect();
    let new_keys: HashSet<&str> = new.iter().map(|n| n.key.as_str()).collect();

    // 1. keys that are gone
    for node in old.iter().filter(|n| !new_keys.contains(n.key.as_str())) {
        ops.push(Op::Remove { path: join_path(parent, slot, &node.key) });
    }

    // 2. keys on both sides: replace on a type change, else recurse
    for node in new {
        let Some(&i) = old_index.get(node.key.as_str()) else { continue };
        let path = join_path(parent, slot, &node.key);
        if old[i].type_ == node.type_ {
            diff_node(&old[i], node, &path, ops);
        } else {
            ops.push(Op::Replace { path, node: node.clone() });
        }
    }

    // 3. order: keep the longest run that is already in order, move the rest, insert the new
    let new_position: HashMap<&str, usize> =
        new.iter().enumerate().map(|(i, n)| (n.key.as_str(), i)).collect();
    let kept: Vec<&str> =
        old.iter().map(|n| n.key.as_str()).filter(|k| new_position.contains_key(k)).collect();
    let positions: Vec<usize> = kept.iter().filter_map(|k| new_position.get(k).copied()).collect();
    let stays: HashSet<usize> =
        longest_increasing_subsequence(&positions).into_iter().map(|i| positions[i]).collect();

    let mut next: Option<&str> = None;
    for (position, node) in new.iter().enumerate().rev() {
        let before = next.map(str::to_owned);
        if old_index.contains_key(node.key.as_str()) {
            if !stays.contains(&position) {
                ops.push(Op::Move { path: join_path(parent, slot, &node.key), before });
            }
        } else {
            ops.push(Op::Insert {
                parent: parent.to_owned(),
                slot: slot.map(str::to_owned),
                before,
                node: node.clone(),
            });
        }
        next = Some(node.key.as_str());
    }
}

/// Indices (into `values`) of a longest strictly increasing subsequence.
fn longest_increasing_subsequence(values: &[usize]) -> Vec<usize> {
    let mut tails: Vec<usize> = Vec::new(); // tails[k] = index of the smallest tail of a run of length k + 1
    let mut previous: Vec<Option<usize>> = vec![None; values.len()];
    for (i, &value) in values.iter().enumerate() {
        let at = tails.partition_point(|&t| values[t] < value);
        previous[i] = at.checked_sub(1).map(|k| tails[k]);
        if at == tails.len() {
            tails.push(i);
        } else {
            tails[at] = i;
        }
    }
    let mut run = Vec::with_capacity(tails.len());
    let mut cursor = tails.last().copied();
    while let Some(i) = cursor {
        run.push(i);
        cursor = previous[i];
    }
    run.reverse();
    run
}

/// An op that cannot be applied: the client answers by calling `view.sync` for a full snapshot.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ApplyError {
    #[error("no node at path `{0}`")]
    NoNodeAtPath(String),
    #[error("no sibling `{key}` to insert or move before, under `{parent}`")]
    NoSibling { parent: String, key: String },
    #[error("key `{key}` already exists under `{parent}`")]
    DuplicateKey { parent: String, key: String },
}

/// Apply ops to a tree. On error the tree may be partially updated: resync with a full snapshot.
///
/// # Errors
/// An [`ApplyError`] when an op addresses a node that does not exist (`remove` is idempotent: removing a
/// missing node is fine).
pub fn apply(root: &mut Node, ops: &[Op]) -> Result<(), ApplyError> {
    for op in ops {
        apply_one(root, op)?;
    }
    Ok(())
}

fn apply_one(root: &mut Node, op: &Op) -> Result<(), ApplyError> {
    match op {
        Op::Set { path, props } => {
            let node = resolve(root, path)?;
            for (name, value) in props {
                if value.is_null() {
                    node.props.remove(name);
                } else {
                    node.props.insert(name.clone(), value.clone());
                }
            }
        }
        Op::On { path, on } => {
            let node = resolve(root, path)?;
            for (event, handler) in on {
                match handler {
                    Some(handler) => {
                        node.on.insert(event.clone(), handler.clone());
                    }
                    None => {
                        node.on.remove(event);
                    }
                }
            }
        }
        Op::Insert { parent, slot, before, node } => {
            let owner = resolve(root, parent)?;
            let list = container_mut(owner, slot.as_deref());
            if list.iter().any(|n| n.key == node.key) {
                return Err(ApplyError::DuplicateKey { parent: parent.clone(), key: node.key.clone() });
            }
            let index = position(list, before.as_deref(), parent)?;
            list.insert(index, node.clone());
        }
        Op::Remove { path } => {
            if let Some((owner_path, slot, key)) = split(path) {
                if let Ok(owner) = resolve(root, owner_path) {
                    let list = container_mut(owner, slot);
                    list.retain(|n| n.key != key);
                    if let Some(slot) = slot {
                        if owner.slots.get(slot).is_some_and(Vec::is_empty) {
                            owner.slots.remove(slot);
                        }
                    }
                }
            }
        }
        Op::Move { path, before } => {
            let (owner_path, slot, key) =
                split(path).ok_or_else(|| ApplyError::NoNodeAtPath(path.clone()))?;
            let owner = resolve(root, owner_path)?;
            let list = container_mut(owner, slot);
            let from = list
                .iter()
                .position(|n| n.key == key)
                .ok_or_else(|| ApplyError::NoNodeAtPath(path.clone()))?;
            let node = list.remove(from);
            let index = position(list, before.as_deref(), owner_path)?;
            list.insert(index, node);
        }
        Op::Replace { path, node } => {
            if split(path).is_none() {
                *root = node.clone();
            } else {
                *resolve(root, path)? = node.clone();
            }
        }
    }
    Ok(())
}

fn position(list: &[Node], before: Option<&str>, parent: &str) -> Result<usize, ApplyError> {
    match before {
        None => Ok(list.len()),
        Some(key) => list
            .iter()
            .position(|n| n.key == key)
            .ok_or_else(|| ApplyError::NoSibling { parent: parent.to_owned(), key: key.to_owned() }),
    }
}

fn container_mut<'a>(node: &'a mut Node, slot: Option<&str>) -> &'a mut Vec<Node> {
    match slot {
        None => &mut node.children,
        Some(name) => node.slots.entry(name.to_owned()).or_default(),
    }
}

/// `main/list/@row/item` -> (`main/list`, Some(`row`), `item`); `main/list` -> (`main`, None, `list`).
fn split(path: &str) -> Option<(&str, Option<&str>, &str)> {
    let (rest, key) = path.rsplit_once('/')?;
    match rest.rsplit_once('/') {
        Some((owner, segment)) if segment.starts_with('@') => Some((owner, Some(&segment[1..]), key)),
        _ => Some((rest, None, key)),
    }
}

fn resolve<'a>(root: &'a mut Node, path: &str) -> Result<&'a mut Node, ApplyError> {
    let missing = || ApplyError::NoNodeAtPath(path.to_owned());
    let mut segments = path.split('/');
    if segments.next() != Some(root.key.as_str()) {
        return Err(missing());
    }
    let mut node = root;
    let mut slot: Option<&str> = None;
    for segment in segments {
        if let Some(name) = segment.strip_prefix('@') {
            slot = Some(name);
            continue;
        }
        let list = match slot.take() {
            None => &mut node.children,
            Some(name) => node.slots.get_mut(name).ok_or_else(missing)?,
        };
        node = list.iter_mut().find(|n| n.key == segment).ok_or_else(missing)?;
    }
    Ok(node)
}

#[cfg(test)]
#[path = "diff_tests.rs"]
mod tests;
