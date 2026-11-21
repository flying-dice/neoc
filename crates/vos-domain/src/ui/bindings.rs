//! Which source fields a tree binds two-way (spec §3.2, §3.4): the `Binding` props that point at
//! `src.<id>.<field>`. The host derives from them the fields a single-row source's `writable` lists, and how a
//! write to each is reconciled (text: merge, scalar: the human wins).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::components;
use super::expr::{Expr, Namespace};
use super::node::{join_path, Node};
use super::spec::BindingKind;

/// One two-way bound field of a source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundField {
    pub source: String,
    pub field: String,
    pub kind: BindingKind,
    /// The path of the node that binds it.
    pub path: String,
}

/// Every `Binding` prop of the tree that points at a source field.
#[must_use]
pub fn bound_fields(root: &Node) -> Vec<BoundField> {
    let mut out = Vec::new();
    visit(root, &root.key, &mut out);
    out
}

fn visit(node: &Node, path: &str, out: &mut Vec<BoundField>) {
    if let Some(spec) = components::lookup(&node.type_) {
        for (name, value) in &node.props {
            let Some(kind) = spec.prop(name).and_then(|p| p.binding) else { continue };
            if let Some((source, field)) = source_field(value) {
                out.push(BoundField { source, field, kind, path: path.to_owned() });
            }
        }
    }
    for (slot, children) in node.containers() {
        for child in children {
            visit(child, &join_path(path, slot, &child.key), out);
        }
    }
}

/// `{"$ref": "src.<id>.<field>"}` -> `(id, field)`.
fn source_field(value: &Value) -> Option<(String, String)> {
    let Ok(Expr::Ref(r)) = Expr::parse(value) else { return None };
    if r.namespace != Namespace::Src {
        return None;
    }
    let (source, field) = r.rest.split_once('.')?;
    Some((source.to_owned(), field.to_owned()))
}

/// The bound fields grouped by source id.
#[must_use]
pub fn writable_by_source(fields: &[BoundField]) -> BTreeMap<String, BTreeSet<String>> {
    let mut by_source: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for f in fields {
        by_source.entry(f.source.clone()).or_default().insert(f.field.clone());
    }
    by_source
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn editor(key: &str, text: Value) -> Node {
        Node::new("TextEditor", key).prop("text", text).prop("label", json!("Note"))
    }

    #[test]
    fn a_text_binding_to_a_source_field_is_found_with_its_kind() {
        let tree = Node::new("Stack", "main")
            .child(editor("body", json!({ "$ref": "src.s3.body" })))
            .child(Node::new("Markdown", "preview").prop("text", json!({ "$ref": "src.s3.body" })));
        let fields = bound_fields(&tree);
        assert_eq!(
            fields,
            vec![BoundField {
                source: "s3".into(),
                field: "body".into(),
                kind: BindingKind::Text,
                path: "main/body".into()
            }],
            "the Markdown preview only reads"
        );
        assert_eq!(writable_by_source(&fields)["s3"], BTreeSet::from(["body".to_owned()]));
    }

    #[test]
    fn client_refs_literals_and_nested_nodes() {
        let tree = Node::new("Stack", "main")
            .child(editor("a", json!({ "$ref": "client.draft" })))
            .child(Node::new("Group", "g").child(editor("b", json!({ "$ref": "src.s1.title" }))));
        let fields = bound_fields(&tree);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].path, "main/g/b");
    }
}
