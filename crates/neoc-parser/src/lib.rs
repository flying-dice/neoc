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
    let module = adapter.walk_module(tree.root_node());
    ParseResult {
        module,
        diagnostics: adapter.diagnostics,
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
            if child.is_error() {
                self.error(child, "syntax error");
                continue;
            }
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
