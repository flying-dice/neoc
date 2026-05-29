//! Expressions, statements, and blocks. Structural and type-independent — the
//! adapter handles every expression kind; type-dependent decisions (e.g. how
//! `+` or a method call lowers) belong to later stages, not here.

use crate::Adapter;
use neoc_ast as ast;
use tree_sitter::Node;

impl Adapter {
    pub(crate) fn walk_block(&mut self, node: Node) -> ast::Block {
        let value_node = self.field(node, "value");
        let mut stmts = Vec::new();
        // A trailing block-like expression with no `;` (e.g. a final `match`)
        // is the block's value, not a statement. tree-sitter parses it as a
        // `block_statement`; we promote the last one here when it carried no
        // semicolon and there is no explicit `value` field.
        let mut tail_is_value = false;
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if Some(child) == value_node {
                continue;
            }
            if child.kind() != "statement" {
                continue;
            }
            let Some(inner) = child.named_child(0) else {
                continue;
            };
            tail_is_value = false;
            let stmt_span = span_of(child);
            match inner.kind() {
                "let_statement" => stmts.push(ast::Stmt::new(
                    ast::StmtKind::Let(self.walk_let(inner)),
                    stmt_span,
                )),
                "expression_statement" => {
                    if let Some(expr) = inner.named_child(0) {
                        let e = self.walk_expr(expr);
                        stmts.push(ast::Stmt::new(ast::StmtKind::Expr(e), stmt_span));
                    }
                }
                "block_statement" => {
                    if let Some(expr) = inner.named_child(0) {
                        // Only value-producing block-likes can be the tail
                        // value; loops evaluate to `()` and stay statements.
                        let yields_value =
                            matches!(expr.kind(), "if_expression" | "match_expression" | "block");
                        let e = self.walk_expr(expr);
                        stmts.push(ast::Stmt::new(ast::StmtKind::Expr(e), stmt_span));
                        tail_is_value = yields_value && !has_semicolon(inner);
                    }
                }
                "item" => {
                    if let Some(item) = self.walk_item(inner) {
                        stmts.push(ast::Stmt::new(ast::StmtKind::Item(item), stmt_span));
                    }
                }
                _ => {}
            }
        }
        let mut value = value_node.map(|v| Box::new(self.walk_expr(v)));
        if value.is_none() && tail_is_value {
            if let Some(ast::Stmt {
                kind: ast::StmtKind::Expr(expr),
                ..
            }) = stmts.pop()
            {
                value = Some(Box::new(expr));
            }
        }
        ast::Block { stmts, value }
    }

    fn walk_let(&mut self, node: Node) -> ast::LetStmt {
        let mutable = node.children(&mut node.walk()).any(|c| c.kind() == "mut");
        let pattern = self
            .field(node, "pattern")
            .map_or(ast::Pattern::Wildcard, |p| self.walk_pattern(p));
        let ty = self.field(node, "type").map(|t| self.walk_type(t));
        let value = self.field(node, "value").map(|v| self.walk_expr(v));
        ast::LetStmt {
            mutable,
            pattern,
            ty,
            value,
        }
    }

    pub(crate) fn walk_expr(&mut self, node: Node) -> ast::Expr {
        let kind = self.walk_expr_kind(node);
        ast::Expr::new(kind, span_of(node))
    }

    #[allow(clippy::too_many_lines)]
    fn walk_expr_kind(&mut self, node: Node) -> ast::ExprKind {
        match node.kind() {
            "integer_literal" => ast::ExprKind::Int(self.text(node).to_string()),
            "float_literal" => ast::ExprKind::Float(self.text(node).to_string()),
            "string_literal" => ast::ExprKind::Str(self.string_value(node)),
            "boolean_literal" => ast::ExprKind::Bool(self.text(node) == "true"),
            "unit_expression" => ast::ExprKind::Unit,
            "path" | "cond_path" => ast::ExprKind::Path(self.path_idents(node)),
            "parenthesized_expression" => node
                .named_child(0)
                .map_or(ast::ExprKind::Unit, |c| self.walk_expr_kind(c)),
            "binary_expression" | "cond_binary_expression" => self.walk_binary(node),
            "unary_expression" | "cond_unary_expression" => {
                let op = match self.field_token(node, "operator") {
                    "!" => ast::UnOp::Not,
                    _ => ast::UnOp::Neg,
                };
                let operand = self.boxed_field(node, "operand");
                ast::ExprKind::Unary { op, operand }
            }
            "range_expression" | "cond_range_expression" => {
                let inclusive = self.field_token(node, "operator") == "..=";
                ast::ExprKind::Range {
                    start: self.boxed_field(node, "start"),
                    end: self.boxed_field(node, "end"),
                    inclusive,
                }
            }
            "try_expression" | "cond_try_expression" => {
                ast::ExprKind::Try(self.boxed_field(node, "operand"))
            }
            "call_expression" | "cond_call_expression" => ast::ExprKind::Call {
                callee: self.boxed_field(node, "function"),
                args: self.walk_args(node),
            },
            "method_call_expression" | "cond_method_call_expression" => ast::ExprKind::MethodCall {
                receiver: self.boxed_field(node, "receiver"),
                method: self
                    .field(node, "method")
                    .map_or_else(String::new, |m| self.text(m).to_string()),
                method_span: self
                    .field(node, "method")
                    .map_or_else(ast::Span::default, span_of),
                args: self.walk_args(node),
            },
            "field_expression" | "cond_field_expression" => {
                let base = self.boxed_field(node, "value");
                let field = self.field(node, "field").map_or(
                    ast::FieldAccess::Named(String::new(), ast::Span::default()),
                    |f| {
                        if f.kind() == "integer_literal" {
                            ast::FieldAccess::Tuple(self.text(f).parse().unwrap_or(0))
                        } else {
                            ast::FieldAccess::Named(self.text(f).to_string(), span_of(f))
                        }
                    },
                );
                ast::ExprKind::Field { base, field }
            }
            "index_expression" | "cond_index_expression" => ast::ExprKind::Index {
                base: self.boxed_field(node, "value"),
                index: self.boxed_field(node, "index"),
            },
            "struct_literal" => self.walk_struct_literal(node),
            "list_literal" => {
                let mut cursor = node.walk();
                ast::ExprKind::List(
                    node.named_children(&mut cursor)
                        .map(|c| self.walk_expr(c))
                        .collect(),
                )
            }
            "assignment_expression" => {
                let op = match self.field_token(node, "operator") {
                    "+=" => ast::AssignOp::Add,
                    "-=" => ast::AssignOp::Sub,
                    "*=" => ast::AssignOp::Mul,
                    "/=" => ast::AssignOp::Div,
                    "%=" => ast::AssignOp::Rem,
                    _ => ast::AssignOp::Assign,
                };
                ast::ExprKind::Assign {
                    target: self.boxed_field(node, "left"),
                    op,
                    value: self.boxed_field(node, "right"),
                }
            }
            "if_expression" => self.walk_if(node),
            "match_expression" => self.walk_match(node),
            "while_expression" => ast::ExprKind::While {
                cond: self.boxed_field(node, "condition"),
                body: self.block_field(node, "body"),
            },
            "for_expression" => ast::ExprKind::For {
                pattern: self
                    .field(node, "pattern")
                    .map_or(ast::Pattern::Wildcard, |p| self.walk_pattern(p)),
                iter: self.boxed_field(node, "iterable"),
                body: self.block_field(node, "body"),
            },
            "loop_expression" => ast::ExprKind::Loop {
                body: self.block_field(node, "body"),
            },
            "closure_expression" => self.walk_closure(node),
            "block" => ast::ExprKind::Block(self.walk_block(node)),
            "return_expression" => {
                ast::ExprKind::Return(node.named_child(0).map(|c| Box::new(self.walk_expr(c))))
            }
            "break_expression" => ast::ExprKind::Break,
            "continue_expression" => ast::ExprKind::Continue,
            other => {
                self.error(node, format!("unsupported expression: {other}"));
                ast::ExprKind::Unit
            }
        }
    }

    fn walk_binary(&mut self, node: Node) -> ast::ExprKind {
        let op = match self.field_token(node, "operator") {
            "||" => ast::BinOp::Or,
            "&&" => ast::BinOp::And,
            "==" => ast::BinOp::Eq,
            "!=" => ast::BinOp::Ne,
            "<" => ast::BinOp::Lt,
            ">" => ast::BinOp::Gt,
            "<=" => ast::BinOp::Le,
            ">=" => ast::BinOp::Ge,
            "+" => ast::BinOp::Add,
            "-" => ast::BinOp::Sub,
            "*" => ast::BinOp::Mul,
            "/" => ast::BinOp::Div,
            _ => ast::BinOp::Rem,
        };
        ast::ExprKind::Binary {
            op,
            lhs: self.boxed_field(node, "left"),
            rhs: self.boxed_field(node, "right"),
        }
    }

    fn walk_struct_literal(&mut self, node: Node) -> ast::ExprKind {
        let path = self
            .field(node, "type")
            .map(|p| self.path_idents(p))
            .unwrap_or_default();
        let base = self
            .field(node, "base")
            .map(|b| Box::new(self.walk_expr(b)));
        let mut fields = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "field_initializer" {
                let name = self.name_field(child);
                let value = self.field(child, "value").map(|v| self.walk_expr(v));
                fields.push(ast::FieldInit { name, value });
            }
        }
        ast::ExprKind::StructLit { path, fields, base }
    }

    fn walk_if(&mut self, node: Node) -> ast::ExprKind {
        let cond = self.boxed_field(node, "condition");
        let then = self.block_field(node, "consequence");
        let else_ = self
            .field(node, "alternative")
            .map(|a| Box::new(self.walk_expr(a)));
        ast::ExprKind::If { cond, then, else_ }
    }

    fn walk_match(&mut self, node: Node) -> ast::ExprKind {
        let scrutinee = self.boxed_field(node, "scrutinee");
        let mut arms = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "match_arm" {
                let pattern = self
                    .field(child, "pattern")
                    .map_or(ast::Pattern::Wildcard, |p| self.walk_pattern(p));
                let guard = self.field(child, "guard").map(|g| self.walk_expr(g));
                let body = self.field(child, "value").map_or_else(
                    || ast::Expr::new(ast::ExprKind::Unit, span_of(child)),
                    |v| self.walk_expr(v),
                );
                arms.push(ast::MatchArm {
                    pattern,
                    guard,
                    body,
                });
            }
        }
        ast::ExprKind::Match { scrutinee, arms }
    }

    fn walk_closure(&mut self, node: Node) -> ast::ExprKind {
        let mut params = Vec::new();
        if let Some(plist) = self.field(node, "parameters") {
            let mut cursor = plist.walk();
            for child in plist.named_children(&mut cursor) {
                if child.kind() == "closure_parameter" {
                    let span = self
                        .field(child, "name")
                        .map_or_else(|| span_of(child), span_of);
                    let name = self.name_field(child);
                    let ty = self.field(child, "type").map(|t| self.walk_type(t));
                    params.push(ast::ClosureParam { name, ty, span });
                }
            }
        }
        let body = self.boxed_field(node, "body");
        ast::ExprKind::Closure { params, body }
    }

    fn walk_args(&mut self, node: Node) -> Vec<ast::Expr> {
        let Some(list) = self.field(node, "arguments") else {
            return Vec::new();
        };
        let mut cursor = list.walk();
        list.named_children(&mut cursor)
            .map(|c| self.walk_expr(c))
            .collect()
    }

    /// A `path` / `cond_path` node's `::`-joined identifiers.
    pub(crate) fn path_idents(&self, node: Node) -> Vec<ast::Ident> {
        let mut cursor = node.walk();
        node.named_children(&mut cursor)
            .filter(|c| c.kind() == "identifier")
            .map(|c| self.text(c).to_string())
            .collect()
    }

    /// The decoded value of a `string_literal` (escapes resolved, quotes
    /// stripped). Raw strings pass through verbatim.
    pub(crate) fn string_value(&mut self, node: Node) -> String {
        let mut out = String::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "string_content" | "raw_string_content" => out.push_str(self.text(child)),
                "escape_sequence" => out.push_str(&decode_escape(self.text(child))),
                _ => {}
            }
        }
        out
    }

    /// The text of an anonymous token in the given field.
    fn field_token(&self, node: Node, name: &str) -> &str {
        self.field(node, name).map_or("", |n| self.text(n))
    }

    fn boxed_field(&mut self, node: Node, name: &str) -> Box<ast::Expr> {
        Box::new(self.field(node, name).map_or_else(
            || ast::Expr::new(ast::ExprKind::Unit, span_of(node)),
            |c| self.walk_expr(c),
        ))
    }

    fn block_field(&mut self, node: Node, name: &str) -> ast::Block {
        self.field(node, name)
            .map_or_else(ast::Block::default, |b| self.walk_block(b))
    }
}

/// Decode a single escape sequence token (including the leading backslash).
fn decode_escape(raw: &str) -> String {
    let mut chars = raw.chars();
    if chars.next() != Some('\\') {
        return raw.to_string();
    }
    match chars.next() {
        Some('n') => "\n".to_string(),
        Some('t') => "\t".to_string(),
        Some('r') => "\r".to_string(),
        Some('0') => "\0".to_string(),
        Some('\\') => "\\".to_string(),
        Some('"') => "\"".to_string(),
        // \xNN and \u{…} are passed through to the emitter, which re-encodes
        // them as Lua escapes; decoding to bytes/chars is deferred.
        _ => raw.to_string(),
    }
}

/// Whether a `block_statement` node carries an explicit trailing `;`.
fn has_semicolon(node: Node) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor).any(|c| c.kind() == ";")
}

/// The byte [`ast::Span`] a CST node occupies in the source.
pub(crate) fn span_of(node: Node) -> ast::Span {
    ast::Span {
        start: node.start_byte(),
        end: node.end_byte(),
    }
}

#[cfg(test)]
mod tests {
    use super::decode_escape;

    #[test]
    fn decodes_recognised_escapes() {
        assert_eq!(decode_escape("\\n"), "\n");
        assert_eq!(decode_escape("\\t"), "\t");
        assert_eq!(decode_escape("\\r"), "\r");
        assert_eq!(decode_escape("\\0"), "\0");
        assert_eq!(decode_escape("\\\\"), "\\");
        assert_eq!(decode_escape("\\\""), "\"");
    }

    #[test]
    fn passes_through_byte_and_unicode_escapes_for_the_emitter() {
        // `\xNN` and `\u{…}` are re-encoded downstream; decode leaves them intact.
        assert_eq!(decode_escape("\\x41"), "\\x41");
        assert_eq!(decode_escape("\\u{1F600}"), "\\u{1F600}");
    }

    #[test]
    fn passes_through_unknown_escape_verbatim() {
        assert_eq!(decode_escape("\\q"), "\\q");
    }

    #[test]
    fn returns_input_unchanged_when_not_an_escape() {
        assert_eq!(decode_escape("plain"), "plain");
        assert_eq!(decode_escape(""), "");
    }
}
