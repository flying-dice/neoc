//! Type expressions: `_type` and friends.

use crate::Adapter;
use neoc_ast as ast;
use tree_sitter::Node;

impl Adapter {
    pub(crate) fn walk_type(&mut self, node: Node) -> ast::Type {
        match node.kind() {
            "unit_type" => ast::Type::Unit,
            "list_type" => {
                let inner = node
                    .named_child(0)
                    .map_or(ast::Type::Unit, |c| self.walk_type(c));
                ast::Type::List(Box::new(inner))
            }
            "self_type" => ast::Type::SelfType,
            "function_type" => {
                let mut cursor = node.walk();
                let mut types: Vec<ast::Type> = node
                    .named_children(&mut cursor)
                    .map(|c| self.walk_type(c))
                    .collect();
                // The grammar lists parameter types then an optional return
                // type as trailing children; the last one is the return type
                // only when an `->` is present. Detect via child count vs the
                // `->` token. Simpler: the return type, if any, is the final
                // child following the `)`. We treat all-but-handled uniformly:
                // here every named child is a type; the return is the last.
                let ret = if node_has_arrow(node) {
                    types.pop().map(Box::new)
                } else {
                    None
                };
                ast::Type::Fn { params: types, ret }
            }
            "type_path" => ast::Type::Path(self.walk_type_path(node)),
            other => {
                self.error(node, format!("unsupported type: {other}"));
                ast::Type::Unit
            }
        }
    }

    pub(crate) fn walk_type_path(&mut self, node: Node) -> ast::TypePath {
        let mut segments = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "identifier" {
                segments.push(ast::TypeSegment {
                    name: self.text(child).to_string(),
                    args: Vec::new(),
                });
            } else if child.kind() == "type_arguments" {
                let args = self.walk_type_arguments(child);
                if let Some(last) = segments.last_mut() {
                    last.args = args;
                }
            }
        }
        ast::TypePath { segments }
    }

    fn walk_type_arguments(&mut self, node: Node) -> Vec<ast::Type> {
        let mut cursor = node.walk();
        node.named_children(&mut cursor)
            .map(|c| self.walk_type(c))
            .collect()
    }
}

/// Whether a `function_type` node contains a `->` token (i.e. a return type).
fn node_has_arrow(node: Node) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor).any(|c| c.kind() == "->")
}
