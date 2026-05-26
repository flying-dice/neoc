//! Goto navigation: definition, type-definition, and implementation.
//!
//! Resolution is name-based against the document's own top-level items: fns,
//! structs, enums, traits, consts, and type aliases. Locals and impl methods
//! are out of scope for this pass — the functions return nothing when the name
//! does not match a top-level declaration.

use neoc_types::Ty;
use tower_lsp::lsp_types::Range;
use tree_sitter::Node;

use crate::cst::{self, node_text};
use crate::position::{byte_range, position_to_byte};

/// A resolved definition target.
///
/// `full_range` spans the whole declaration (the item the editor highlights on
/// peek); `name_range` is the declared name the editor places the cursor on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Definition {
    pub full_range: Range,
    pub name_range: Range,
}

/// The definition for the identifier at `position`, if it names a top-level
/// declaration.
///
/// Resolves against the document's top-level items (fns, structs, enums,
/// traits, consts, type aliases). Returns `None` when the name is unknown or
/// the cursor already sits on the defining name itself.
#[must_use]
pub fn goto_definition(
    source: &str,
    position: tower_lsp::lsp_types::Position,
) -> Option<Definition> {
    let tree = cst::parse(source);
    let byte = position_to_byte(source, position);
    let name = cst::identifier_at(&tree, source, byte)?;
    let decl = cst::find_decl(&tree, source, name)?;
    // Don't report a no-op jump when the cursor already sits on the defining
    // name itself.
    if decl.name_node.byte_range().contains(&byte) {
        return None;
    }
    Some(Definition {
        full_range: byte_range(source, decl.node.start_byte(), decl.node.end_byte()),
        name_range: byte_range(
            source,
            decl.name_node.start_byte(),
            decl.name_node.end_byte(),
        ),
    })
}

/// The declaration of the *type* of the value named at `position`.
///
/// Resolves the identifier to a value entry in the symbol table and, when its
/// type is a named type (struct/enum/trait/alias), jumps to that type's
/// declaration. Limited to top-level values (e.g. consts) — locals and
/// parameters are not tracked yet — and to named types; returns `None`
/// otherwise.
#[must_use]
pub fn goto_type_definition(
    source: &str,
    position: tower_lsp::lsp_types::Position,
) -> Option<Definition> {
    let tree = cst::parse(source);
    let byte = position_to_byte(source, position);
    let name = cst::identifier_at(&tree, source, byte)?;
    let table = neoc_types::collect(&neoc_parser::parse(source).module);
    let Ty::Named {
        name: type_name, ..
    } = table.values.get(name)?
    else {
        return None;
    };
    let decl = cst::find_decl(&tree, source, type_name)?;
    Some(Definition {
        full_range: byte_range(source, decl.node.start_byte(), decl.node.end_byte()),
        name_range: byte_range(
            source,
            decl.name_node.start_byte(),
            decl.name_node.end_byte(),
        ),
    })
}

/// Every `impl` block that targets the type, or implements the trait, named at
/// `position`.
///
/// The identifier under the cursor is matched against each impl block's target
/// type (`impl Point`) and its implemented trait (`impl Show for Point`), so
/// clicking a type lists its inherent impls and clicking a trait lists its
/// implementations. Empty when the cursor is not on an identifier or nothing
/// targets it.
#[must_use]
pub fn goto_implementation(
    source: &str,
    position: tower_lsp::lsp_types::Position,
) -> Vec<Definition> {
    let tree = cst::parse(source);
    let byte = position_to_byte(source, position);
    let Some(name) = cst::identifier_at(&tree, source, byte) else {
        return Vec::new();
    };
    let root = tree.root_node();
    let mut out = Vec::new();
    for item in root.named_children(&mut root.walk()) {
        if item.kind() != "item" {
            continue;
        }
        for decl in item.named_children(&mut item.walk()) {
            if decl.kind() == "impl_block" && impl_targets(decl, source, name) {
                let target = decl.child_by_field_name("type").unwrap_or(decl);
                out.push(Definition {
                    full_range: byte_range(source, decl.start_byte(), decl.end_byte()),
                    name_range: byte_range(source, target.start_byte(), target.end_byte()),
                });
            }
        }
    }
    out
}

/// Whether an `impl_block` targets the type or implements the trait named
/// `name`, comparing the head identifier of its `type` and `trait` paths.
fn impl_targets(imp: Node, source: &str, name: &str) -> bool {
    let head = |field| {
        imp.child_by_field_name(field)
            .and_then(|node| type_head_name(node, source))
    };
    head("type") == Some(name) || head("trait") == Some(name)
}

/// The head identifier of a `type_path` (`Point` in `Point<T>`), or `None` for
/// types with no leading name (`[T]`, `()`, `fn(...)`, `self`).
fn type_head_name<'a>(ty: Node, source: &'a str) -> Option<&'a str> {
    if ty.kind() != "type_path" {
        return None;
    }
    let head = ty
        .named_children(&mut ty.walk())
        .find(|c| c.kind() == "identifier")?;
    node_text(&head, source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::byte_to_position;

    fn position_of(src: &str, needle: &str, occurrence: usize) -> tower_lsp::lsp_types::Position {
        let mut from = 0usize;
        let mut idx = 0usize;
        for _ in 0..occurrence {
            let i = src[from..].find(needle).expect("occurrence") + from;
            idx = i;
            from = i + 1;
        }
        byte_to_position(src, idx + 1)
    }

    #[test]
    fn call_resolves_to_fn_decl() {
        let src = "fn fib(n: number) -> number { fib(n) }\n";
        // Cursor on the recursive call `fib`.
        let call = src.rfind("fib").unwrap();
        let pos = byte_to_position(src, call + 1);
        let def = goto_definition(src, pos).expect("definition");
        // The decl name `fib` starts at byte 3; the item spans the whole fn.
        assert_eq!(def.name_range, byte_range(src, 3, 6));
        assert_eq!(def.full_range.start, byte_to_position(src, 0));
    }

    #[test]
    fn struct_literal_resolves_to_struct() {
        let src = "struct Point { x: number }\nfn make() -> Point { Point { x: 1 } }\n";
        let pos = position_of(src, "Point", 2);
        let def = goto_definition(src, pos).expect("definition");
        assert_eq!(def.name_range, byte_range(src, 7, 12)); // "Point" in the decl
    }

    #[test]
    fn cursor_on_definition_is_none() {
        let src = "fn fib(n: number) -> number { n }\n";
        let pos = byte_to_position(src, 4); // on the `fib` of the decl
        assert_eq!(goto_definition(src, pos), None);
    }

    #[test]
    fn unknown_name_is_none() {
        let src = "fn main() { 1 + 1 }\n";
        let pos = byte_to_position(src, 0);
        assert_eq!(goto_definition(src, pos), None);
    }

    #[test]
    fn type_definition_of_a_const_jumps_to_its_type() {
        let src = "struct Point { x: number }\nconst ORIGIN: Point = Point { x: 0 };\nfn m() -> number { ORIGIN.x }\n";
        // Cursor on the use of `ORIGIN`, whose type is `Point`.
        let def =
            goto_type_definition(src, position_of(src, "ORIGIN", 2)).expect("type definition");
        assert_eq!(def.name_range, byte_range(src, 7, 12)); // "Point" in the struct decl
    }

    #[test]
    fn type_definition_of_a_type_name_is_none() {
        // A type name is not a value, so it has no type to jump to.
        let src = "struct Point { x: number }\nfn make() -> Point { Point { x: 1 } }\n";
        assert_eq!(
            goto_type_definition(src, position_of(src, "Point", 2)),
            None
        );
    }

    #[test]
    fn implementation_finds_inherent_impl_of_a_type() {
        let src = "struct Counter { n: number }\nimpl Counter { fn inc(self) -> number { self.n } }\nfn m(c: Counter) -> number { c.n }\n";
        // Cursor on the type name in the struct declaration.
        let impls = goto_implementation(src, position_of(src, "Counter", 1));
        assert_eq!(impls.len(), 1, "{impls:?}");
        // The selection points at `Counter` in `impl Counter`.
        let impl_at = src.find("impl Counter").unwrap() + "impl ".len();
        assert_eq!(
            impls[0].name_range,
            byte_range(src, impl_at, impl_at + "Counter".len())
        );
    }

    #[test]
    fn implementation_finds_trait_impls() {
        let src = "trait Show { fn show(self) -> string; }\nstruct P { x: number }\nimpl Show for P { fn show(self) -> string { \"p\" } }\n";
        // Cursor on the trait name lists its implementations.
        let impls = goto_implementation(src, position_of(src, "Show", 1));
        assert_eq!(impls.len(), 1, "{impls:?}");
    }

    #[test]
    fn implementation_of_unimplemented_type_is_empty() {
        let src = "struct Lonely { x: number }\nfn m(l: Lonely) -> number { l.x }\n";
        assert!(goto_implementation(src, position_of(src, "Lonely", 1)).is_empty());
    }
}
