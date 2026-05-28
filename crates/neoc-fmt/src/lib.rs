//! Source formatter for neoc. Rewrites `.neoc` source to a canonical form.
//!
//! Formatting works off the tree-sitter concrete syntax tree, not the typed
//! AST, so comments survive the round trip. The CST is walked with a structured
//! pretty-printer that reproduces the hand-written corpus style: two-space
//! indent, opening brace on the same line, one blank line between top-level
//! items, spaces around binary operators and after `,` / `:`.

use std::fmt;

use tree_sitter::{Node, Parser, Tree};

/// The reason a source string could not be formatted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FmtError {
    /// The tree-sitter parser could not be constructed with the neoc grammar.
    Parser,
    /// The source did not parse cleanly — the tree contains an `ERROR` or
    /// `MISSING` node. The formatter never rewrites a file it cannot parse,
    /// so this is surfaced rather than risk emitting garbage.
    Parse,
}

impl fmt::Display for FmtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FmtError::Parser => f.write_str("could not load the neoc grammar"),
            FmtError::Parse => f.write_str("source has a syntax error"),
        }
    }
}

impl std::error::Error for FmtError {}

/// Formats neoc source into its canonical form.
///
/// The returned string ends in exactly one trailing newline. Formatting is
/// idempotent: `format(&format(src)?)?` equals `format(src)?`.
///
/// # Errors
///
/// Returns [`FmtError::Parser`] if the tree-sitter grammar cannot be loaded,
/// or [`FmtError::Parse`] if the source contains a syntax error. The formatter
/// never emits partial or invalid output for an unparsable file.
pub fn format(source: &str) -> Result<String, FmtError> {
    let mut parser = Parser::new();
    parser
        .set_language(&neoc_grammar::LANGUAGE.into())
        .map_err(|_| FmtError::Parser)?;
    let tree: Tree = parser.parse(source, None).ok_or(FmtError::Parse)?;
    let root = tree.root_node();
    if root.has_error() {
        return Err(FmtError::Parse);
    }

    let mut printer = Printer::new(source);
    printer.source_file(root);
    Ok(printer.finish())
}

/// Holds the rendered output, the indent level, and a borrow of the source so
/// leaf tokens and comments can be sliced out by byte span.
struct Printer<'a> {
    src: &'a [u8],
    out: String,
    indent: usize,
}

impl<'a> Printer<'a> {
    fn new(source: &'a str) -> Self {
        Printer {
            src: source.as_bytes(),
            out: String::new(),
            indent: 0,
        }
    }

    /// The verbatim source text of a node.
    fn text(&self, node: Node) -> &'a str {
        std::str::from_utf8(&self.src[node.byte_range()]).unwrap_or_default()
    }

    fn push(&mut self, s: &str) {
        self.out.push_str(s);
    }

    /// Writes the current indent at the start of a fresh line. Four spaces per
    /// level, matching rustfmt — neoc is Rust-shaped, so its canonical form
    /// mirrors Rust's.
    fn write_indent(&mut self) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
    }

    fn newline(&mut self) {
        self.out.push('\n');
    }

    fn finish(mut self) -> String {
        while self.out.ends_with('\n') {
            self.out.pop();
        }
        self.out.push('\n');
        self.out
    }

    // ---- comment plumbing ----------------------------------------------

    /// A comment node renders verbatim apart from being re-indented.
    fn is_comment(kind: &str) -> bool {
        matches!(kind, "line_comment" | "block_comment" | "doc_comment")
    }

    /// Emits a comment node, re-indenting each interior line of a block
    /// comment to the current level.
    fn comment(&mut self, node: Node) {
        let text = self.text(node);
        let mut lines = text.lines();
        if let Some(first) = lines.next() {
            self.push(first.trim_end());
        }
        for line in lines {
            self.newline();
            self.write_indent();
            self.push(line.trim_end());
        }
    }

    /// Whether two byte offsets are separated by a blank line in the source —
    /// used to preserve a single blank line the author put between items.
    fn blank_between(&self, end: usize, start: usize) -> bool {
        if start <= end {
            return false;
        }
        let mut newlines = 0u8;
        for &b in &self.src[end..start] {
            if b == b'\n' {
                newlines += 1;
                if newlines >= 2 {
                    return true;
                }
            }
        }
        false
    }

    // ---- top level -----------------------------------------------------

    fn source_file(&mut self, node: Node) {
        let mut cursor = node.walk();
        let mut prev_end: Option<usize> = None;
        let mut prev_was_item = false;
        let mut prev_was_use = false;
        for child in node.named_children(&mut cursor) {
            let is_item = !Self::is_comment(child.kind());
            let is_use = is_item && Self::item_is_use(child);
            if let Some(end) = prev_end {
                // One blank line between top-level items, and wherever the
                // author left a blank. A comment hugging the item below it
                // (no blank between) is leading and stays attached.
                // Consecutive `use` declarations form a tight block: any blank
                // between them is collapsed (Rust-style import grouping), even
                // one the author wrote. A comment between two uses still
                // separates (it is not itself a `use`).
                let both_use = prev_was_use && is_use;
                let force_blank = prev_was_item && is_item && !both_use;
                if !both_use && (force_blank || self.blank_between(end, child.start_byte())) {
                    self.newline();
                }
            }
            self.write_indent();
            if Self::is_comment(child.kind()) {
                self.comment(child);
            } else {
                self.item(child);
            }
            self.newline();
            prev_end = Some(child.end_byte());
            prev_was_item = is_item;
            prev_was_use = is_use;
        }
    }

    /// Whether an `item` node wraps a bare `use_declaration` with no attributes.
    /// Attributed items break onto their own block and never group tightly.
    fn item_is_use(node: Node) -> bool {
        let mut cursor = node.walk();
        let mut saw_use = false;
        for child in node.children(&mut cursor) {
            match child.kind() {
                "use_declaration" => saw_use = true,
                "attribute" => return false,
                _ => {}
            }
        }
        saw_use
    }

    // ---- items ---------------------------------------------------------

    fn item(&mut self, node: Node) {
        // `item` wraps attributes, an optional `pub`, and one inner item.
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            let kind = child.kind();
            if Self::is_comment(kind) {
                self.comment(child);
                self.newline();
                self.write_indent();
            } else if kind == "attribute" {
                self.attribute(child);
                self.newline();
                self.write_indent();
            } else if kind == "pub" {
                self.push("pub ");
            } else {
                self.inner_item(child);
            }
        }
    }

    fn inner_item(&mut self, node: Node) {
        match node.kind() {
            "use_declaration" => self.use_declaration(node),
            "const_declaration" => self.const_declaration(node),
            "type_alias" => self.type_alias(node),
            "struct_declaration" => self.struct_declaration(node),
            "enum_declaration" => self.enum_declaration(node),
            "trait_declaration" => self.trait_declaration(node),
            "impl_block" => self.impl_block(node),
            "function_declaration" => self.function_declaration(node),
            "extern_block" => self.extern_block(node),
            other => self.fallback(node, other),
        }
    }

    fn attribute(&mut self, node: Node) {
        self.push("#[");
        let mut cursor = node.walk();
        let items: Vec<Node> = node
            .children(&mut cursor)
            .filter(|c| c.kind() == "attribute_item")
            .collect();
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                self.push(", ");
            }
            self.attribute_item(*item);
        }
        self.push("]");
    }

    fn attribute_item(&mut self, node: Node) {
        let name = node.child_by_field_name("name").unwrap_or(node);
        self.push(self.text(name));
        let mut cursor = node.walk();
        let args: Vec<Node> = node
            .children(&mut cursor)
            .filter(|c| {
                matches!(c.kind(), "attribute_item" | "attribute_named_arg")
                    || Self::is_literal(c.kind())
            })
            .collect();
        let has_parens = node.children(&mut node.walk()).any(|c| c.kind() == "(");
        if has_parens {
            self.push("(");
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    self.push(", ");
                }
                match arg.kind() {
                    "attribute_item" => self.attribute_item(*arg),
                    "attribute_named_arg" => self.attribute_named_arg(*arg),
                    _ => self.push(self.text(*arg)),
                }
            }
            self.push(")");
        }
    }

    fn attribute_named_arg(&mut self, node: Node) {
        if let Some(key) = node.child_by_field_name("key") {
            self.push(self.text(key));
        }
        self.push(" = ");
        if let Some(value) = node.child_by_field_name("value") {
            self.push(self.text(value));
        }
    }

    fn use_declaration(&mut self, node: Node) {
        self.push("use ");
        if let Some(path) = node.child_by_field_name("path") {
            self.push(self.text(path));
        }
        if let Some(alias) = node.child_by_field_name("alias") {
            self.push(" as ");
            self.push(self.text(alias));
        }
        self.push(";");
    }

    fn const_declaration(&mut self, node: Node) {
        self.push("const ");
        self.push(self.text(node.child_by_field_name("name").unwrap()));
        self.push(": ");
        self.ty(node.child_by_field_name("type").unwrap());
        self.push(" = ");
        self.expr(node.child_by_field_name("value").unwrap());
        self.push(";");
    }

    fn type_alias(&mut self, node: Node) {
        self.push("type ");
        self.push(self.text(node.child_by_field_name("name").unwrap()));
        if let Some(generics) = node.child_by_field_name("generics") {
            self.generic_params(generics);
        }
        self.push(" = ");
        self.ty(node.child_by_field_name("value").unwrap());
        self.push(";");
    }

    fn struct_declaration(&mut self, node: Node) {
        self.push("struct ");
        self.push(self.text(node.child_by_field_name("name").unwrap()));
        if let Some(generics) = node.child_by_field_name("generics") {
            self.generic_params(generics);
        }
        if let Some(element) = node.child_by_field_name("element") {
            // newtype: `struct Wrapper(number);`
            self.push("(");
            self.ty(element);
            self.push(");");
        } else if let Some(body) = node.child_by_field_name("body") {
            self.push(" ");
            self.field_declaration_list(body);
        } else {
            // unit struct
            self.push(";");
        }
    }

    fn field_declaration_list(&mut self, node: Node) {
        let mut cursor = node.walk();
        let fields: Vec<Node> = node
            .named_children(&mut cursor)
            .filter(|c| c.kind() == "field_declaration" || Self::is_comment(c.kind()))
            .collect();
        if fields.is_empty() {
            self.push("{}");
            return;
        }
        // rustfmt-style: a named field list is always expanded, one field per
        // line with a trailing comma.
        self.push("{");
        self.indent += 1;
        for field in &fields {
            self.newline();
            self.write_indent();
            if Self::is_comment(field.kind()) {
                self.comment(*field);
            } else {
                self.field_declaration(*field);
                self.push(",");
            }
        }
        self.indent -= 1;
        self.newline();
        self.write_indent();
        self.push("}");
    }

    fn field_declaration(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "attribute" {
                self.attribute(child);
                self.push(" ");
            }
        }
        self.push(self.text(node.child_by_field_name("name").unwrap()));
        self.push(": ");
        self.ty(node.child_by_field_name("type").unwrap());
    }

    fn enum_declaration(&mut self, node: Node) {
        self.push("enum ");
        self.push(self.text(node.child_by_field_name("name").unwrap()));
        if let Some(generics) = node.child_by_field_name("generics") {
            self.generic_params(generics);
        }
        let mut cursor = node.walk();
        let variants: Vec<Node> = node
            .named_children(&mut cursor)
            .filter(|c| c.kind() == "variant" || Self::is_comment(c.kind()))
            .collect();
        if variants.is_empty() {
            self.push(" {}");
            return;
        }
        // rustfmt-style: one variant per line with a trailing comma.
        self.push(" {");
        self.indent += 1;
        for v in &variants {
            self.newline();
            self.write_indent();
            if Self::is_comment(v.kind()) {
                self.comment(*v);
            } else {
                self.variant(*v);
                self.push(",");
            }
        }
        self.indent -= 1;
        self.newline();
        self.write_indent();
        self.push("}");
    }

    fn variant(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "attribute" {
                self.attribute(child);
                self.push(" ");
            }
        }
        self.push(self.text(node.child_by_field_name("name").unwrap()));
        if let Some(body) = node.child_by_field_name("body") {
            self.push(" ");
            self.field_declaration_list(body);
        } else {
            // Possibly a tuple variant: collect the type children.
            let mut tc = node.walk();
            let types: Vec<Node> = node
                .named_children(&mut tc)
                .filter(|c| Self::is_type(c.kind()))
                .collect();
            let has_paren = node.children(&mut node.walk()).any(|c| c.kind() == "(");
            if has_paren {
                self.push("(");
                for (i, t) in types.iter().enumerate() {
                    if i > 0 {
                        self.push(", ");
                    }
                    self.ty(*t);
                }
                self.push(")");
            }
        }
    }

    fn trait_declaration(&mut self, node: Node) {
        self.push("trait ");
        self.push(self.text(node.child_by_field_name("name").unwrap()));
        if let Some(generics) = node.child_by_field_name("generics") {
            self.generic_params(generics);
        }
        self.push(" ");
        self.braced_items(node, "trait_item", Printer::trait_item);
    }

    fn trait_item(&mut self, node: Node) {
        self.function_signature(node);
        if let Some(body) = node.child_by_field_name("body") {
            self.push(" ");
            self.block_multiline(body);
        } else {
            self.push(";");
        }
    }

    fn impl_block(&mut self, node: Node) {
        self.push("impl");
        if let Some(generics) = node.child_by_field_name("generics") {
            self.generic_params(generics);
        }
        self.push(" ");
        if let Some(trait_ref) = node.child_by_field_name("trait") {
            self.ty(trait_ref);
            self.push(" for ");
        }
        self.ty(node.child_by_field_name("type").unwrap());
        self.push(" ");
        self.braced_items(node, "impl_item", Printer::impl_item);
    }

    fn impl_item(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "attribute" => {
                    self.attribute(child);
                    self.newline();
                    self.write_indent();
                }
                "function_declaration" => self.function_declaration(child),
                k if Self::is_comment(k) => {
                    self.comment(child);
                    self.newline();
                    self.write_indent();
                }
                _ => {}
            }
        }
    }

    /// Formats an `extern { … }` block (`lang/EXTERN.md`): a region of bodyless
    /// `mod`/`struct`/`impl`/`fn` items.
    fn extern_block(&mut self, node: Node) {
        self.push("extern {");
        self.extern_items(node);
        self.newline();
        self.write_indent();
        self.push("}");
    }

    /// Formats the `extern_item` (and comment) children of an `extern_block` or
    /// `extern_mod`, one per indented line.
    fn extern_items(&mut self, node: Node) {
        let mut cursor = node.walk();
        let items: Vec<Node> = node
            .named_children(&mut cursor)
            .filter(|c| c.kind() == "extern_item" || Self::is_comment(c.kind()))
            .collect();
        self.indent += 1;
        for item in &items {
            self.newline();
            self.write_indent();
            if Self::is_comment(item.kind()) {
                self.comment(*item);
            } else {
                self.extern_item(*item);
            }
        }
        self.indent -= 1;
    }

    /// Formats one `extern_item`: its attributes, optional `pub`, and one inner
    /// form (`extern_mod` / `struct_declaration` / `extern_impl` / `extern_fn`).
    fn extern_item(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "attribute" => {
                    self.attribute(child);
                    self.newline();
                    self.write_indent();
                }
                "pub" => self.push("pub "),
                "extern_mod" => self.extern_mod(child),
                "struct_declaration" => self.struct_declaration(child),
                "extern_impl" => self.extern_impl(child),
                "extern_fn" => {
                    self.function_signature(child);
                    self.push(";");
                }
                k if Self::is_comment(k) => self.comment(child),
                _ => {}
            }
        }
    }

    fn extern_mod(&mut self, node: Node) {
        self.push("mod ");
        self.push(self.text(node.child_by_field_name("name").unwrap()));
        self.push(" {");
        self.extern_items(node);
        self.newline();
        self.write_indent();
        self.push("}");
    }

    fn extern_impl(&mut self, node: Node) {
        self.push("impl");
        if let Some(generics) = node.child_by_field_name("generics") {
            self.generic_params(generics);
        }
        self.push(" ");
        self.ty(node.child_by_field_name("type").unwrap());
        self.push(" {");
        let mut cursor = node.walk();
        let methods: Vec<Node> = node
            .named_children(&mut cursor)
            .filter(|c| c.kind() == "extern_method" || Self::is_comment(c.kind()))
            .collect();
        self.indent += 1;
        for m in &methods {
            self.newline();
            self.write_indent();
            if Self::is_comment(m.kind()) {
                self.comment(*m);
            } else {
                self.extern_method(*m);
            }
        }
        self.indent -= 1;
        self.newline();
        self.write_indent();
        self.push("}");
    }

    fn extern_method(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "attribute" => {
                    self.attribute(child);
                    self.newline();
                    self.write_indent();
                }
                "pub" => self.push("pub "),
                k if Self::is_comment(k) => self.comment(child),
                _ => {}
            }
        }
        self.function_signature(node);
        self.push(";");
    }

    fn function_declaration(&mut self, node: Node) {
        self.function_signature(node);
        self.push(" ");
        self.block_multiline(node.child_by_field_name("body").unwrap());
    }

    fn function_signature(&mut self, node: Node) {
        self.push("fn ");
        self.push(self.text(node.child_by_field_name("name").unwrap()));
        if let Some(generics) = node.child_by_field_name("generics") {
            self.generic_params(generics);
        }
        self.parameter_list(node.child_by_field_name("parameters").unwrap());
        if let Some(ret) = node.child_by_field_name("return_type") {
            self.push(" -> ");
            self.ty(ret);
        }
        if let Some(where_clause) = node.child_by_field_name("where") {
            self.where_clause(where_clause);
        }
    }

    fn parameter_list(&mut self, node: Node) {
        self.push("(");
        let mut cursor = node.walk();
        let params: Vec<Node> = node
            .named_children(&mut cursor)
            .filter(|c| c.kind() == "parameter" || c.kind() == "self_parameter")
            .collect();
        for (i, param) in params.iter().enumerate() {
            if i > 0 {
                self.push(", ");
            }
            if param.kind() == "self_parameter" {
                self.push("self");
            } else {
                self.push(self.text(param.child_by_field_name("name").unwrap()));
                self.push(": ");
                self.ty(param.child_by_field_name("type").unwrap());
            }
        }
        self.push(")");
    }

    // ---- generics ------------------------------------------------------

    fn generic_params(&mut self, node: Node) {
        self.push("<");
        let mut cursor = node.walk();
        let params: Vec<Node> = node
            .named_children(&mut cursor)
            .filter(|c| c.kind() == "generic_param")
            .collect();
        for (i, param) in params.iter().enumerate() {
            if i > 0 {
                self.push(", ");
            }
            self.push(self.text(param.child_by_field_name("name").unwrap()));
            if let Some(bounds) = param.child_by_field_name("bounds") {
                self.push(": ");
                self.bounds(bounds);
            }
        }
        self.push(">");
    }

    fn bounds(&mut self, node: Node) {
        let mut cursor = node.walk();
        let paths: Vec<Node> = node
            .named_children(&mut cursor)
            .filter(|c| c.kind() == "type_path")
            .collect();
        for (i, path) in paths.iter().enumerate() {
            if i > 0 {
                self.push(" + ");
            }
            self.ty(*path);
        }
    }

    fn where_clause(&mut self, node: Node) {
        self.push(" where ");
        let mut cursor = node.walk();
        let preds: Vec<Node> = node
            .named_children(&mut cursor)
            .filter(|c| c.kind() == "where_predicate")
            .collect();
        for (i, pred) in preds.iter().enumerate() {
            if i > 0 {
                self.push(", ");
            }
            self.push(self.text(pred.child_by_field_name("type").unwrap()));
            self.push(": ");
            self.bounds(pred.child_by_field_name("bounds").unwrap());
        }
    }

    // ---- types ---------------------------------------------------------

    fn is_type(kind: &str) -> bool {
        matches!(
            kind,
            "unit_type" | "list_type" | "function_type" | "self_type" | "type_path"
        )
    }

    fn ty(&mut self, node: Node) {
        match node.kind() {
            "unit_type" => self.push("()"),
            "self_type" => self.push("Self"),
            "list_type" => {
                self.push("[");
                self.ty(node.named_child(0).unwrap());
                self.push("]");
            }
            "function_type" => {
                self.push("fn(");
                let mut cursor = node.walk();
                let types: Vec<Node> = node
                    .named_children(&mut cursor)
                    .filter(|c| Self::is_type(c.kind()))
                    .collect();
                // The return type, if present, is the last type child preceded
                // by `->`. Detect it by scanning for the `->` token.
                let ret_present = node.children(&mut node.walk()).any(|c| c.kind() == "->");
                let split = if ret_present {
                    types.len() - 1
                } else {
                    types.len()
                };
                for (i, t) in types.iter().take(split).enumerate() {
                    if i > 0 {
                        self.push(", ");
                    }
                    self.ty(*t);
                }
                self.push(")");
                if ret_present {
                    self.push(" -> ");
                    self.ty(*types.last().unwrap());
                }
            }
            "type_path" => self.type_path(node),
            other => self.fallback(node, other),
        }
    }

    fn type_path(&mut self, node: Node) {
        // `Module::Foo<Bar>::baz`: identifiers joined by `::`, each maybe
        // carrying `<...>` type arguments.
        let mut cursor = node.walk();
        let mut first = true;
        for child in node.children(&mut cursor) {
            match child.kind() {
                "identifier" => {
                    if !first {
                        self.push("::");
                    }
                    first = false;
                    self.push(self.text(child));
                }
                "type_arguments" => self.type_arguments(child),
                _ => {}
            }
        }
    }

    fn type_arguments(&mut self, node: Node) {
        self.push("<");
        let mut cursor = node.walk();
        let types: Vec<Node> = node
            .named_children(&mut cursor)
            .filter(|c| Self::is_type(c.kind()))
            .collect();
        for (i, t) in types.iter().enumerate() {
            if i > 0 {
                self.push(", ");
            }
            self.ty(*t);
        }
        self.push(">");
    }

    // ---- blocks & statements -------------------------------------------

    /// A braced block: function body, control-flow body (`if`/`else`/`while`/
    /// `for`/`loop`/match arm), or block expression. Always multi-line like
    /// rustfmt — only an empty block stays `{}`.
    fn block_multiline(&mut self, node: Node) {
        self.block(node);
    }

    fn block(&mut self, node: Node) {
        let mut cursor = node.walk();
        let children: Vec<Node> = node
            .named_children(&mut cursor)
            .filter(|c| {
                c.kind() == "statement" || Self::is_comment(c.kind()) || Self::is_expr(c.kind())
            })
            .collect();
        if children.is_empty() {
            self.push("{}");
            return;
        }
        self.push("{");
        self.indent += 1;
        let mut prev_end: Option<usize> = None;
        for child in &children {
            if let Some(end) = prev_end {
                if self.blank_between(end, child.start_byte()) {
                    self.newline();
                }
            }
            self.newline();
            self.write_indent();
            if Self::is_comment(child.kind()) {
                self.comment(*child);
            } else if child.kind() == "statement" {
                self.statement(*child);
            } else {
                // trailing block value expression
                self.expr(*child);
            }
            prev_end = Some(child.end_byte());
        }
        self.indent -= 1;
        self.newline();
        self.write_indent();
        self.push("}");
    }

    fn statement(&mut self, node: Node) {
        // `statement` wraps one of let / item / block_statement / expr_stmt.
        let inner = node.named_child(0).unwrap_or(node);
        match inner.kind() {
            "let_statement" => self.let_statement(inner),
            "item" => self.item(inner),
            "block_statement" => self.block_statement(inner),
            "expression_statement" => {
                self.expr(inner.named_child(0).unwrap());
                self.push(";");
            }
            other => self.fallback(inner, other),
        }
    }

    fn block_statement(&mut self, node: Node) {
        // A block-like expression used for effect; trailing `;` is dropped.
        self.expr(node.named_child(0).unwrap());
    }

    fn let_statement(&mut self, node: Node) {
        self.push("let ");
        if node.children(&mut node.walk()).any(|c| c.kind() == "mut") {
            self.push("mut ");
        }
        self.pattern(node.child_by_field_name("pattern").unwrap());
        if let Some(ty) = node.child_by_field_name("type") {
            self.push(": ");
            self.ty(ty);
        }
        if let Some(value) = node.child_by_field_name("value") {
            self.push(" = ");
            self.expr(value);
        }
        self.push(";");
    }

    // ---- expressions ---------------------------------------------------

    fn is_expr(kind: &str) -> bool {
        matches!(
            kind,
            "assignment_expression"
                | "binary_expression"
                | "unary_expression"
                | "range_expression"
                | "try_expression"
                | "call_expression"
                | "method_call_expression"
                | "field_expression"
                | "index_expression"
                | "if_expression"
                | "match_expression"
                | "while_expression"
                | "for_expression"
                | "loop_expression"
                | "closure_expression"
                | "block"
                | "struct_literal"
                | "list_literal"
                | "parenthesized_expression"
                | "unit_expression"
                | "return_expression"
                | "break_expression"
                | "continue_expression"
                | "path"
                | "integer_literal"
                | "float_literal"
                | "string_literal"
                | "boolean_literal"
        )
    }

    fn is_literal(kind: &str) -> bool {
        matches!(
            kind,
            "integer_literal" | "float_literal" | "string_literal" | "boolean_literal"
        )
    }

    fn expr(&mut self, node: Node) {
        match node.kind() {
            "assignment_expression" | "binary_expression" | "cond_binary_expression" => {
                self.expr(node.child_by_field_name("left").unwrap());
                self.push(" ");
                self.push(self.text(node.child_by_field_name("operator").unwrap()));
                self.push(" ");
                self.expr(node.child_by_field_name("right").unwrap());
            }
            "unary_expression" | "cond_unary_expression" => {
                self.push(self.text(node.child_by_field_name("operator").unwrap()));
                self.expr(node.child_by_field_name("operand").unwrap());
            }
            "range_expression" | "cond_range_expression" => {
                self.expr(node.child_by_field_name("start").unwrap());
                self.push(self.text(node.child_by_field_name("operator").unwrap()));
                self.expr(node.child_by_field_name("end").unwrap());
            }
            "try_expression" | "cond_try_expression" => {
                self.expr(node.child_by_field_name("operand").unwrap());
                self.push("?");
            }
            "call_expression" | "cond_call_expression" => {
                self.expr(node.child_by_field_name("function").unwrap());
                self.argument_list(node.child_by_field_name("arguments").unwrap());
            }
            "method_call_expression" | "cond_method_call_expression" => {
                self.expr(node.child_by_field_name("receiver").unwrap());
                self.push(".");
                self.push(self.text(node.child_by_field_name("method").unwrap()));
                self.argument_list(node.child_by_field_name("arguments").unwrap());
            }
            "field_expression" | "cond_field_expression" => {
                self.expr(node.child_by_field_name("value").unwrap());
                self.push(".");
                self.push(self.text(node.child_by_field_name("field").unwrap()));
            }
            "index_expression" | "cond_index_expression" => {
                self.expr(node.child_by_field_name("value").unwrap());
                self.push("[");
                self.expr(node.child_by_field_name("index").unwrap());
                self.push("]");
            }
            "if_expression" => self.if_expression(node),
            "match_expression" => self.match_expression(node),
            "while_expression" => self.while_expression(node),
            "for_expression" => self.for_expression(node),
            "loop_expression" => self.loop_expression(node),
            "closure_expression" => self.closure_expression(node),
            "block" => self.block(node),
            "struct_literal" => self.struct_literal(node),
            "list_literal" => self.list_literal(node),
            "parenthesized_expression" => {
                self.push("(");
                self.expr(node.named_child(0).unwrap());
                self.push(")");
            }
            "unit_expression" => self.push("()"),
            "return_expression" => {
                self.push("return");
                if let Some(value) = node.named_child(0) {
                    self.push(" ");
                    self.expr(value);
                }
            }
            "break_expression" => self.push("break"),
            "continue_expression" => self.push("continue"),
            "path" | "cond_path" => self.push(self.text(node)),
            "integer_literal" | "float_literal" | "boolean_literal" | "string_literal" => {
                self.push(self.text(node));
            }
            other => self.fallback(node, other),
        }
    }

    fn argument_list(&mut self, node: Node) {
        self.push("(");
        let mut cursor = node.walk();
        let args: Vec<Node> = node
            .named_children(&mut cursor)
            .filter(|c| Self::is_expr(c.kind()))
            .collect();
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                self.push(", ");
            }
            self.expr(*arg);
        }
        self.push(")");
    }

    fn list_literal(&mut self, node: Node) {
        self.push("[");
        let mut cursor = node.walk();
        let items: Vec<Node> = node
            .named_children(&mut cursor)
            .filter(|c| Self::is_expr(c.kind()))
            .collect();
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                self.push(", ");
            }
            self.expr(*item);
        }
        self.push("]");
    }

    fn if_expression(&mut self, node: Node) {
        self.push("if ");
        self.expr(node.child_by_field_name("condition").unwrap());
        self.push(" ");
        self.block(node.child_by_field_name("consequence").unwrap());
        if let Some(alt) = node.child_by_field_name("alternative") {
            self.push(" else ");
            if alt.kind() == "if_expression" {
                self.if_expression(alt);
            } else {
                self.block(alt);
            }
        }
    }

    fn match_expression(&mut self, node: Node) {
        self.push("match ");
        self.expr(node.child_by_field_name("scrutinee").unwrap());
        self.push(" {");
        self.indent += 1;
        let mut cursor = node.walk();
        let arms: Vec<Node> = node
            .named_children(&mut cursor)
            .filter(|c| c.kind() == "match_arm" || Self::is_comment(c.kind()))
            .collect();
        for arm in &arms {
            self.newline();
            self.write_indent();
            if Self::is_comment(arm.kind()) {
                self.comment(*arm);
            } else {
                self.match_arm(*arm);
                self.push(",");
            }
        }
        self.indent -= 1;
        self.newline();
        self.write_indent();
        self.push("}");
    }

    fn match_arm(&mut self, node: Node) {
        self.pattern(node.child_by_field_name("pattern").unwrap());
        if let Some(guard) = node.child_by_field_name("guard") {
            self.push(" if ");
            self.expr(guard);
        }
        self.push(" => ");
        self.expr(node.child_by_field_name("value").unwrap());
    }

    fn while_expression(&mut self, node: Node) {
        self.push("while ");
        self.expr(node.child_by_field_name("condition").unwrap());
        self.push(" ");
        self.block(node.child_by_field_name("body").unwrap());
    }

    fn for_expression(&mut self, node: Node) {
        self.push("for ");
        self.pattern(node.child_by_field_name("pattern").unwrap());
        self.push(" in ");
        self.expr(node.child_by_field_name("iterable").unwrap());
        self.push(" ");
        self.block(node.child_by_field_name("body").unwrap());
    }

    fn loop_expression(&mut self, node: Node) {
        self.push("loop ");
        self.block(node.child_by_field_name("body").unwrap());
    }

    fn closure_expression(&mut self, node: Node) {
        let params = node.child_by_field_name("parameters").unwrap();
        self.push("|");
        let mut cursor = params.walk();
        let cps: Vec<Node> = params
            .named_children(&mut cursor)
            .filter(|c| c.kind() == "closure_parameter")
            .collect();
        for (i, cp) in cps.iter().enumerate() {
            if i > 0 {
                self.push(", ");
            }
            self.push(self.text(cp.child_by_field_name("name").unwrap()));
            if let Some(ty) = cp.child_by_field_name("type") {
                self.push(": ");
                self.ty(ty);
            }
        }
        self.push("| ");
        self.expr(node.child_by_field_name("body").unwrap());
    }

    fn struct_literal(&mut self, node: Node) {
        self.push(self.text(node.child_by_field_name("type").unwrap()));
        let mut cursor = node.walk();
        let inits: Vec<Node> = node
            .named_children(&mut cursor)
            .filter(|c| c.kind() == "field_initializer")
            .collect();
        let base = node.child_by_field_name("base");
        if inits.is_empty() && base.is_none() {
            self.push(" {}");
            return;
        }
        self.push(" { ");
        for (i, init) in inits.iter().enumerate() {
            if i > 0 {
                self.push(", ");
            }
            let name = init.child_by_field_name("name").unwrap();
            self.push(self.text(name));
            if let Some(value) = init.child_by_field_name("value") {
                self.push(": ");
                self.expr(value);
            }
        }
        if let Some(base) = base {
            if !inits.is_empty() {
                self.push(", ");
            }
            self.push("..");
            self.expr(base);
        }
        self.push(" }");
    }

    // ---- patterns ------------------------------------------------------

    fn pattern(&mut self, node: Node) {
        match node.kind() {
            "or_pattern" => {
                let mut cursor = node.walk();
                let pats: Vec<Node> = node
                    .named_children(&mut cursor)
                    .filter(|c| Self::is_pattern(c.kind()))
                    .collect();
                for (i, p) in pats.iter().enumerate() {
                    if i > 0 {
                        self.push(" | ");
                    }
                    self.pattern(*p);
                }
            }
            "wildcard_pattern" => self.push("_"),
            "literal_pattern" | "binding_pattern" | "path_pattern" => self.push(self.text(node)),
            "struct_pattern" => self.struct_pattern(node),
            "tuple_pattern" => self.tuple_pattern(node),
            other => self.fallback(node, other),
        }
    }

    fn is_pattern(kind: &str) -> bool {
        matches!(
            kind,
            "or_pattern"
                | "wildcard_pattern"
                | "literal_pattern"
                | "struct_pattern"
                | "tuple_pattern"
                | "path_pattern"
                | "binding_pattern"
        )
    }

    fn struct_pattern(&mut self, node: Node) {
        self.push(self.text(node.child_by_field_name("type").unwrap()));
        self.push(" { ");
        let mut cursor = node.walk();
        let fields: Vec<Node> = node
            .named_children(&mut cursor)
            .filter(|c| c.kind() == "field_pattern")
            .collect();
        for (i, field) in fields.iter().enumerate() {
            if i > 0 {
                self.push(", ");
            }
            let name = field.child_by_field_name("name").unwrap();
            self.push(self.text(name));
            if let Some(pat) = field.child_by_field_name("pattern") {
                self.push(": ");
                self.pattern(pat);
            }
        }
        let rest = node.children(&mut node.walk()).any(|c| c.kind() == "..");
        if rest {
            if !fields.is_empty() {
                self.push(", ");
            }
            self.push("..");
        }
        self.push(" }");
    }

    fn tuple_pattern(&mut self, node: Node) {
        self.push(self.text(node.child_by_field_name("type").unwrap()));
        self.push("(");
        let mut cursor = node.walk();
        let pats: Vec<Node> = node
            .named_children(&mut cursor)
            .filter(|c| Self::is_pattern(c.kind()))
            .collect();
        for (i, p) in pats.iter().enumerate() {
            if i > 0 {
                self.push(", ");
            }
            self.pattern(*p);
        }
        self.push(")");
    }

    // ---- shared helpers ------------------------------------------------

    /// Renders the `{ … }` body of a trait or impl: each item on its own line,
    /// blank-line separated, with interleaved comments preserved.
    fn braced_items(&mut self, node: Node, item_kind: &str, mut emit: impl FnMut(&mut Self, Node)) {
        let mut cursor = node.walk();
        let items: Vec<Node> = node
            .named_children(&mut cursor)
            .filter(|c| c.kind() == item_kind || Self::is_comment(c.kind()))
            .collect();
        if items.is_empty() {
            self.push("{}");
            return;
        }
        self.push("{");
        self.indent += 1;
        let mut prev_end: Option<usize> = None;
        let mut prev_was_item = false;
        for item in &items {
            let is_item = item.kind() == item_kind;
            if let Some(end) = prev_end {
                // Items get a blank line between them; a comment hugging the
                // item below stays attached.
                if (prev_was_item && is_item) || self.blank_between(end, item.start_byte()) {
                    self.newline();
                }
            }
            self.newline();
            self.write_indent();
            if Self::is_comment(item.kind()) {
                self.comment(*item);
            } else {
                emit(self, *item);
            }
            prev_end = Some(item.end_byte());
            prev_was_item = is_item;
        }
        self.indent -= 1;
        self.newline();
        self.write_indent();
        self.push("}");
    }

    /// Emits a node verbatim. Reached only for a grammar node kind the walker
    /// does not handle; during development this keeps output faithful while a
    /// debug assertion makes the gap loud.
    fn fallback(&mut self, node: Node, kind: &str) {
        debug_assert!(false, "neoc-fmt: unhandled node kind `{kind}`");
        let _ = kind;
        self.push(self.text(node));
    }
}
