//! Patterns. A lone identifier becomes `Binding`; name resolution later
//! reinterprets it as a unit struct/variant when it resolves to one.

use crate::Adapter;
use neoc_ast as ast;
use tree_sitter::Node;

impl Adapter {
    pub(crate) fn walk_pattern(&mut self, node: Node) -> ast::Pattern {
        match node.kind() {
            "wildcard_pattern" => ast::Pattern::Wildcard,
            "literal_pattern" => node
                .named_child(0)
                .map_or(ast::Pattern::Wildcard, |lit| self.literal_pattern(lit)),
            "binding_pattern" => {
                node.named_child(0)
                    .map_or(ast::Pattern::Wildcard, |id| ast::Pattern::Binding {
                        name: self.text(id).to_string(),
                        span: crate::expr::span_of(id),
                    })
            }
            "path_pattern" => ast::Pattern::Path(self.pattern_path_idents(node)),
            "struct_pattern" => self.walk_struct_pattern(node),
            "tuple_pattern" => self.walk_tuple_pattern(node),
            "or_pattern" => {
                let mut cursor = node.walk();
                ast::Pattern::Or(
                    node.named_children(&mut cursor)
                        .map(|c| self.walk_pattern(c))
                        .collect(),
                )
            }
            other => {
                self.error(node, format!("unsupported pattern: {other}"));
                ast::Pattern::Wildcard
            }
        }
    }

    fn literal_pattern(&mut self, node: Node) -> ast::Pattern {
        let lit = match node.kind() {
            "integer_literal" => ast::LiteralPat::Int(self.text(node).to_string()),
            "float_literal" => ast::LiteralPat::Float(self.text(node).to_string()),
            "string_literal" => ast::LiteralPat::Str(self.string_value(node)),
            "boolean_literal" => ast::LiteralPat::Bool(self.text(node) == "true"),
            other => {
                self.error(node, format!("unsupported literal pattern: {other}"));
                return ast::Pattern::Wildcard;
            }
        };
        ast::Pattern::Literal(lit)
    }

    fn walk_struct_pattern(&mut self, node: Node) -> ast::Pattern {
        let path = self
            .field(node, "type")
            .map(|p| self.pattern_path_idents(p))
            .unwrap_or_default();
        let rest = node.children(&mut node.walk()).any(|c| c.kind() == "..");
        let mut fields = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "field_pattern" {
                let span = self
                    .field(child, "name")
                    .map_or_else(|| crate::expr::span_of(child), crate::expr::span_of);
                let name = self.name_field(child);
                let pattern = self.field(child, "pattern").map(|p| self.walk_pattern(p));
                fields.push(ast::FieldPat {
                    name,
                    pattern,
                    span,
                });
            }
        }
        ast::Pattern::Struct { path, fields, rest }
    }

    fn walk_tuple_pattern(&mut self, node: Node) -> ast::Pattern {
        let path = self
            .field(node, "type")
            .map(|p| self.pattern_path_idents(p))
            .unwrap_or_default();
        let mut elems = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind().ends_with("_pattern") || child.kind() == "or_pattern" {
                elems.push(self.walk_pattern(child));
            }
        }
        ast::Pattern::Tuple { path, elems }
    }

    /// The identifiers of a `pattern_path` (`Color::Red` → `[Color, Red]`).
    fn pattern_path_idents(&self, node: Node) -> Vec<ast::Ident> {
        // Both `path_pattern` (`Color::Red`) and `pattern_path` (the head of a
        // struct/tuple pattern) hold their identifiers as direct named
        // children — collect them straight off `node`.
        let mut cursor = node.walk();
        node.named_children(&mut cursor)
            .filter(|c| c.kind() == "identifier")
            .map(|c| self.text(c).to_string())
            .collect()
    }
}
