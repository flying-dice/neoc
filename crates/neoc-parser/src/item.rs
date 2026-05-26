//! Top-level items: use / const / type / struct / enum / trait / impl / fn / ext.

use crate::Adapter;
use neoc_ast as ast;
use tree_sitter::Node;

impl Adapter {
    pub(crate) fn walk_item(&mut self, node: Node) -> Option<ast::Item> {
        let attrs = self.walk_attributes(node);
        let vis = if has_token(node, "pub") {
            ast::Visibility::Public
        } else {
            ast::Visibility::Private
        };
        let decl = node
            .named_children(&mut node.walk())
            .find(|c| !matches!(c.kind(), "attribute" | "doc_comment"))?;
        let kind = match decl.kind() {
            "use_declaration" => ast::ItemKind::Use(self.walk_use(decl)),
            "const_declaration" => ast::ItemKind::Const(self.walk_const(decl)),
            "type_alias" => ast::ItemKind::TypeAlias(self.walk_type_alias(decl)),
            "struct_declaration" => ast::ItemKind::Struct(self.walk_struct(decl)),
            "enum_declaration" => ast::ItemKind::Enum(self.walk_enum(decl)),
            "trait_declaration" => ast::ItemKind::Trait(self.walk_trait(decl)),
            "impl_block" => ast::ItemKind::Impl(self.walk_impl(decl)),
            "function_declaration" => ast::ItemKind::Fn(self.walk_fn(decl, Vec::new())),
            "ext_block" => ast::ItemKind::Ext(self.walk_ext_items(decl)),
            "extern_block" => self.walk_extern(decl),
            other => {
                self.error(decl, format!("unsupported item: {other}"));
                return None;
            }
        };
        Some(ast::Item {
            attrs,
            vis,
            kind,
            span: crate::expr::span_of(node),
        })
    }

    fn walk_use(&mut self, node: Node) -> ast::UseDecl {
        let path = self
            .field(node, "path")
            .map(|p| self.idents(p))
            .unwrap_or_default();
        let alias = self.field(node, "alias").map(|a| self.text(a).to_string());
        ast::UseDecl { path, alias }
    }

    fn walk_const(&mut self, node: Node) -> ast::ConstDecl {
        let name = self.name_field(node);
        let ty = self
            .field(node, "type")
            .map_or(ast::Type::Unit, |t| self.walk_type(t));
        let value = self.field(node, "value").map_or_else(
            || ast::Expr::new(ast::ExprKind::Unit, crate::expr::span_of(node)),
            |v| self.walk_expr(v),
        );
        ast::ConstDecl { name, ty, value }
    }

    fn walk_type_alias(&mut self, node: Node) -> ast::TypeAlias {
        let name = self.name_field(node);
        let generics = self.walk_generics(node);
        let value = self
            .field(node, "value")
            .map_or(ast::Type::Unit, |t| self.walk_type(t));
        ast::TypeAlias {
            name,
            generics,
            value,
        }
    }

    fn walk_struct(&mut self, node: Node) -> ast::StructDecl {
        let name = self.name_field(node);
        let generics = self.walk_generics(node);
        let body = if let Some(body) = self.field(node, "body") {
            ast::StructBody::Named(self.walk_field_decls(body))
        } else if let Some(elem) = self.field(node, "element") {
            ast::StructBody::Tuple(self.walk_type(elem))
        } else {
            ast::StructBody::Unit
        };
        ast::StructDecl {
            name,
            generics,
            body,
        }
    }

    fn walk_field_decls(&mut self, list: Node) -> Vec<ast::Field> {
        let mut fields = Vec::new();
        let mut cursor = list.walk();
        for child in list.named_children(&mut cursor) {
            if child.kind() == "field_declaration" {
                let attrs = self.walk_attributes(child);
                let name = self.name_field(child);
                let ty = self
                    .field(child, "type")
                    .map_or(ast::Type::Unit, |t| self.walk_type(t));
                fields.push(ast::Field { attrs, name, ty });
            }
        }
        fields
    }

    fn walk_enum(&mut self, node: Node) -> ast::EnumDecl {
        let name = self.name_field(node);
        let generics = self.walk_generics(node);
        let mut variants = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "variant" {
                variants.push(self.walk_variant(child));
            }
        }
        ast::EnumDecl {
            name,
            generics,
            variants,
        }
    }

    fn walk_variant(&mut self, node: Node) -> ast::Variant {
        let attrs = self.walk_attributes(node);
        let name = self.name_field(node);
        let body = if let Some(body) = self.field(node, "body") {
            ast::VariantBody::Named(self.walk_field_decls(body))
        } else if let Some(tuple) = tuple_types(node) {
            ast::VariantBody::Tuple(tuple.into_iter().map(|t| self.walk_type(t)).collect())
        } else {
            ast::VariantBody::Unit
        };
        ast::Variant { attrs, name, body }
    }

    fn walk_trait(&mut self, node: Node) -> ast::TraitDecl {
        let name = self.name_field(node);
        let generics = self.walk_generics(node);
        let supertraits = self
            .field(node, "supertraits")
            .map(|b| self.walk_bounds(b))
            .unwrap_or_default();
        let mut items = Vec::new();
        let mut assoc_types = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() != "trait_item" {
                continue;
            }
            if let Some(at) = child
                .named_children(&mut child.walk())
                .find(|c| c.kind() == "associated_type")
            {
                assoc_types.push(self.name_field(at));
            } else {
                let sig = self.walk_fn_sig(child);
                let default = self.field(child, "body").map(|b| self.walk_block(b));
                items.push(ast::TraitItem { sig, default });
            }
        }
        ast::TraitDecl {
            name,
            generics,
            supertraits,
            assoc_types,
            items,
        }
    }

    fn walk_impl(&mut self, node: Node) -> ast::ImplBlock {
        let generics = self.walk_generics(node);
        let trait_ = self.field(node, "trait").map(|t| self.walk_type_path(t));
        let ty = self
            .field(node, "type")
            .map_or(ast::Type::Unit, |t| self.walk_type(t));
        let mut items = Vec::new();
        let mut assoc_bindings = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() != "impl_item" {
                continue;
            }
            let mut ic = child.walk();
            let inner: Vec<Node> = child.named_children(&mut ic).collect();
            if let Some(f) = inner.iter().find(|c| c.kind() == "function_declaration") {
                let attrs = self.walk_attributes(child);
                items.push(self.walk_fn(*f, attrs));
            } else if let Some(b) = inner.iter().find(|c| c.kind() == "associated_type_binding") {
                let name = self.name_field(*b);
                let value = self
                    .field(*b, "value")
                    .map_or(ast::Type::Unit, |t| self.walk_type(t));
                assoc_bindings.push((name, value));
            }
        }
        ast::ImplBlock {
            generics,
            trait_,
            ty,
            items,
            assoc_bindings,
        }
    }

    fn walk_fn(&mut self, node: Node, attrs: Vec<ast::Attribute>) -> ast::FnDecl {
        let sig = self.walk_fn_sig(node);
        let body = self
            .field(node, "body")
            .map_or_else(ast::Block::default, |b| self.walk_block(b));
        ast::FnDecl { attrs, sig, body }
    }

    pub(crate) fn walk_fn_sig(&mut self, node: Node) -> ast::FnSig {
        let name = self.name_field(node);
        let generics = self.walk_generics(node);
        let params = self
            .field(node, "parameters")
            .map(|p| self.walk_params(p))
            .unwrap_or_default();
        let ret = self.field(node, "return_type").map(|t| self.walk_type(t));
        let where_clause = self
            .field(node, "where")
            .map(|w| self.walk_where(w))
            .unwrap_or_default();
        ast::FnSig {
            name,
            generics,
            params,
            ret,
            where_clause,
        }
    }

    fn walk_params(&mut self, node: Node) -> Vec<ast::Param> {
        let mut params = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "self_parameter" => params.push(ast::Param::Receiver),
                "parameter" => {
                    let span = self
                        .field(child, "name")
                        .map_or_else(|| crate::expr::span_of(child), crate::expr::span_of);
                    let name = self.name_field(child);
                    let ty = self
                        .field(child, "type")
                        .map_or(ast::Type::Unit, |t| self.walk_type(t));
                    params.push(ast::Param::Named { name, ty, span });
                }
                _ => {}
            }
        }
        params
    }

    /// Builds an `Extern { host, items }` from an `extern "<host>" { ... }`
    /// block. The `host` field is the string-literal host; the items reuse the
    /// same `ext_item` walker as un-hosted `ext`.
    fn walk_extern(&mut self, node: Node) -> ast::ItemKind {
        let host = self
            .field(node, "host")
            .map_or_else(String::new, |h| self.string_value(h));
        let items = self.walk_ext_items(node);
        ast::ItemKind::Extern { host, items }
    }

    /// Walks the `ext_item` children of an `ext_block` or `extern_block`.
    fn walk_ext_items(&mut self, node: Node) -> Vec<ast::ExtItem> {
        let mut out = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() != "ext_item" {
                continue;
            }
            // An ext_item is one of: fn signature, `type` decl, `const` decl.
            if child
                .named_children(&mut child.walk())
                .any(|c| c.kind() == "parameter_list")
                || has_token(child, "fn")
            {
                out.push(ast::ExtItem::Fn(self.walk_fn_sig(child)));
            } else if has_token(child, "type") {
                out.push(ast::ExtItem::Type {
                    name: self.name_field(child),
                    generics: self.walk_generics(child),
                });
            } else if has_token(child, "const") {
                let name = self.name_field(child);
                let ty = self
                    .field(child, "type")
                    .map_or(ast::Type::Unit, |t| self.walk_type(t));
                out.push(ast::ExtItem::Const { name, ty });
            }
        }
        out
    }

    // ---- shared helpers ----------------------------------------------------

    pub(crate) fn walk_generics(&mut self, node: Node) -> Vec<ast::GenericParam> {
        let Some(gp) = self.field(node, "generics") else {
            return Vec::new();
        };
        let mut params = Vec::new();
        let mut cursor = gp.walk();
        for child in gp.named_children(&mut cursor) {
            if child.kind() == "generic_param" {
                let name = self.name_field(child);
                let bounds = self
                    .field(child, "bounds")
                    .map(|b| self.walk_bounds(b))
                    .unwrap_or_default();
                params.push(ast::GenericParam { name, bounds });
            }
        }
        params
    }

    fn walk_bounds(&mut self, node: Node) -> Vec<ast::TypePath> {
        let mut cursor = node.walk();
        node.named_children(&mut cursor)
            .filter(|c| c.kind() == "type_path")
            .map(|c| self.walk_type_path(c))
            .collect()
    }

    fn walk_where(&mut self, node: Node) -> Vec<ast::WherePredicate> {
        let mut preds = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "where_predicate" {
                let ty = self
                    .field(child, "type")
                    .map_or_else(String::new, |t| self.text(t).to_string());
                let bounds = self
                    .field(child, "bounds")
                    .map(|b| self.walk_bounds(b))
                    .unwrap_or_default();
                preds.push(ast::WherePredicate { ty, bounds });
            }
        }
        preds
    }

    /// The `::`-joined identifiers of a `use_path`.
    fn idents(&self, node: Node) -> Vec<ast::Ident> {
        let mut cursor = node.walk();
        node.named_children(&mut cursor)
            .filter(|c| c.kind() == "identifier")
            .map(|c| self.text(c).to_string())
            .collect()
    }

    pub(crate) fn walk_attributes(&mut self, node: Node) -> Vec<ast::Attribute> {
        let mut attrs = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "attribute" {
                attrs.push(self.walk_attribute(child));
            }
        }
        attrs
    }

    fn walk_attribute(&mut self, node: Node) -> ast::Attribute {
        let mut items = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "attribute_item" {
                items.push(self.walk_attr_item(child));
            }
        }
        ast::Attribute { items }
    }

    fn walk_attr_item(&mut self, node: Node) -> ast::AttrItem {
        let name = self.name_field(node);
        let mut args = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "attribute_item" => args.push(ast::AttrArg::Item(self.walk_attr_item(child))),
                "attribute_named_arg" => {
                    let key = self
                        .field(child, "key")
                        .map_or_else(String::new, |k| self.text(k).to_string());
                    let value = self
                        .field(child, "value")
                        .map_or(ast::AttrArg::Bool(false), |v| self.walk_attr_literal(v));
                    args.push(ast::AttrArg::Named {
                        name: key,
                        value: Box::new(value),
                    });
                }
                _ => {
                    if let Some(arg) = self.walk_attr_literal_opt(child) {
                        args.push(arg);
                    }
                }
            }
        }
        ast::AttrItem { name, args }
    }

    /// Parses a literal node into an `AttrArg`, defaulting unknown nodes to a
    /// `false` boolean (only ever reached on a malformed tree).
    fn walk_attr_literal(&mut self, node: Node) -> ast::AttrArg {
        self.walk_attr_literal_opt(node)
            .unwrap_or(ast::AttrArg::Bool(false))
    }

    /// Parses a literal node (`integer`/`float`/`string`/`boolean`) into an
    /// `AttrArg`, or `None` for any other node kind.
    fn walk_attr_literal_opt(&mut self, node: Node) -> Option<ast::AttrArg> {
        match node.kind() {
            "integer_literal" => Some(ast::AttrArg::Int(self.text(node).to_string())),
            "float_literal" => Some(ast::AttrArg::Float(self.text(node).to_string())),
            "string_literal" => Some(ast::AttrArg::Str(self.string_value(node))),
            "boolean_literal" => Some(ast::AttrArg::Bool(self.text(node) == "true")),
            _ => None,
        }
    }
}

/// Whether `node` has a direct child that is the given anonymous token.
fn has_token(node: Node, token: &str) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor).any(|c| c.kind() == token)
}

/// The type nodes inside a tuple variant's `( … )`, if the variant is a tuple.
fn tuple_types(node: Node<'_>) -> Option<Vec<Node<'_>>> {
    let mut cursor = node.walk();
    let has_paren = node.children(&mut cursor).any(|c| c.kind() == "(");
    if !has_paren {
        return None;
    }
    let mut cursor = node.walk();
    let types: Vec<Node> = node
        .named_children(&mut cursor)
        .filter(|c| c.kind().ends_with("_type") || c.kind() == "type_path")
        .collect();
    Some(types)
}
