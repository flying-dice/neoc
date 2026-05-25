# `#[link(name = "x")]` key-value form

- **Status:** done (subsumed by extern FFI — `#[link(name = "x")]` now parses)
- **Area:** `crates/neocc-grammar`, `crates/neocc-parser`, `crates/neocc-ast`
- **Priority:** P2

## Goal

Support the Rust-faithful `#[link(name = "x")]` key-value attribute argument. neoc's
attribute grammar currently has no `key = value` form, so the module-binding `ext`
block uses positional `#[link("x")]` as the interim form.

Add a `name = literal` form to `_attribute_arg`, surface it in the AST as a new
`AttrArg::Named` / `AttrArg::KeyValue` variant, wire the parser adapter, and thread it
through every stage (types, fmt, codegen, bindgen).

## Acceptance

- [ ] Grammar: `_attribute_arg` accepts `IDENT = literal` alongside the positional form.
- [ ] AST: new `AttrArg` variant carrying the key and literal value.
- [ ] Parser adapter maps the tree-sitter node onto the new variant.
- [ ] `link_attr()` (and any callers) accept both positional and `name = "x"` forms.
- [ ] Tests: parse, fmt round-trip, and codegen emit for both forms.

## Notes

- Positional `#[link("x")]` stays valid; the key-value form is additive.
