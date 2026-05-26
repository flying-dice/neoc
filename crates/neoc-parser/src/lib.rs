//! CST → typed AST adapter. Walks the tree-sitter tree from `neoc-grammar`
//! and builds the `neoc-ast` types.
//!
//! Structure: one `walk_*` function per construct family, dispatched from
//! [`Adapter`]. Unsupported node kinds push a [`Diagnostic`] and are skipped
//! rather than panicking, so the adapter degrades gracefully while families
//! are still being filled in. Tests assert the produced AST against an
//! expected value — the parser stage is verified independently of the emitter.

use neoc_ast as ast;
use tree_sitter::{Node, Parser};

mod expr;
mod item;
mod pattern;
mod ty;

/// A problem found while building the AST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    pub start: usize,
    pub end: usize,
}

/// The result of adapting a source file.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseResult {
    pub module: ast::Module,
    pub diagnostics: Vec<Diagnostic>,
}

impl ParseResult {
    #[must_use]
    pub fn ok(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

/// Parse neoc source into a typed AST plus any diagnostics.
#[must_use]
pub fn parse(source: &str) -> ParseResult {
    let mut parser = Parser::new();
    parser
        .set_language(&neoc_grammar::LANGUAGE.into())
        .expect("load neoc grammar");
    let tree = parser
        .parse(source, None)
        .expect("tree-sitter never returns None here");
    let mut adapter = Adapter::new(source);
    let root = tree.root_node();
    let module = adapter.walk_module(root);
    // Surface every syntax error in the tree, not just those at the top level:
    // tree-sitter recovers from a malformed construct with an `ERROR`/missing
    // node nested anywhere (e.g. a bad `struct` field), and a silently accepted
    // parse would let malformed source reach the checker and emitter.
    if root.has_error() {
        collect_syntax_errors(root, &mut adapter.diagnostics);
    }
    ParseResult {
        module,
        diagnostics: adapter.diagnostics,
    }
}

/// Walks the whole tree, pushing one diagnostic per `ERROR` or missing node.
///
/// `has_error` on the root tells us a syntax error exists somewhere; this
/// locates each. An `ERROR` node's subtree is not descended into — one
/// diagnostic marks the whole malformed region rather than a cascade.
fn collect_syntax_errors(node: Node, diagnostics: &mut Vec<Diagnostic>) {
    if node.is_error() || node.is_missing() {
        diagnostics.push(Diagnostic {
            message: "syntax error".to_string(),
            start: node.start_byte(),
            end: node.end_byte(),
        });
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_syntax_errors(child, diagnostics);
    }
}

/// Owns the source text and accumulates diagnostics while walking. Owning the
/// source (rather than borrowing) decouples the adapter from the tree-sitter
/// tree lifetime: node-derived text comes back tied to `&self`, and only the
/// node-returning [`Adapter::field`] helper is generic over the node lifetime.
pub(crate) struct Adapter {
    src: String,
    diagnostics: Vec<Diagnostic>,
}

impl Adapter {
    fn new(src: &str) -> Self {
        Self {
            src: src.to_string(),
            diagnostics: Vec::new(),
        }
    }

    /// The source text a node spans.
    fn text(&self, node: Node) -> &str {
        &self.src[node.byte_range()]
    }

    /// The single child of `node` in the given field, if present.
    #[allow(clippy::unused_self)] // method form keeps call sites uniform (`self.field(...)`)
    fn field<'t>(&self, node: Node<'t>, name: &str) -> Option<Node<'t>> {
        node.child_by_field_name(name)
    }

    fn error(&mut self, node: Node, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            message: message.into(),
            start: node.start_byte(),
            end: node.end_byte(),
        });
    }

    fn walk_module(&mut self, root: Node) -> ast::Module {
        let mut items = Vec::new();
        let mut cursor = root.walk();
        for child in root.named_children(&mut cursor) {
            // Syntax errors anywhere in the tree are surfaced by the tree-wide
            // scan in `parse`; here we only build items from the valid nodes.
            if child.kind() == "item" {
                if let Some(item) = self.walk_item(child) {
                    items.push(item);
                }
            }
        }
        ast::Module { items }
    }

    /// The identifier text of a node's `name` field.
    fn name_field(&mut self, node: Node) -> ast::Ident {
        if let Some(n) = self.field(node, "name") {
            self.text(n).to_string()
        } else {
            self.error(node, "missing name");
            String::new()
        }
    }
}
