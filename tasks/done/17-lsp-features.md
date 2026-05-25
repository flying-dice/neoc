# LSP — hover, completion, goto, symbols

- **Status:** done
- **Area:** `crates/neocc-lsp`, `crates/neocc-types` (additive query API)
- **Commits:** `8558194` (types re-exports), `ae60f5a` (CST + position scaffold),
  `3041380` (the four features)

## Outcome

All four IDE features landed, built on the tree-sitter CST for position→node
(`position_to_byte` → `named_descendant_for_byte_range`, walking up to the
`identifier` of a `path`) and the `neocc-types` symbol table for resolution —
no AST-span refactor:

- **Document symbols / outline** — structs/enums/traits/impls nest their
  fields/variants/methods.
- **Goto-definition** — `LocationLink` for fns, structs, enums, traits, consts,
  type aliases.
- **Hover** — neoc signatures from the symbol table (`fn fib(number) -> number`,
  `struct Point { … }`, variants, consts); `None` for unknown locals.
- **Completion** — top-level names + keywords; enum variants after `Type::`;
  struct fields after `recv.` (context from the byte-prefix, since half-typed
  `::`/`.` parses with errors).

`neocc-types` additively re-exports its symbol-table info structs. 28 tests in
`neocc-lsp`. Capabilities registered in `initialize`.

**Still deferred to `lsp-spans.md`:** precise per-node ranges for type/emit
diagnostics (they still fall back to line 0).

## Goal

Build on the `tower-lsp` server with features that read the typed model from
`neocc-types`:

- **Hover** — the type of the symbol under the cursor (`Ty` from `check`).
- **Completion** — names in scope, struct fields, enum variants, methods.
- **Goto-definition** — jump to a declaration.
- **Document symbols / outline** — items and members for the outline view.

## Depends on

`lsp-spans.md` — most of these need a position→node mapping, i.e. spans on the
AST. Land spans first, then these become position lookups over the typed tree.

## Acceptance

- [ ] Hover over a binding shows its type.
- [ ] Completion offers in-scope names and member accesses.
- [ ] Goto-definition resolves to the declaring item.
- [ ] Handler logic unit-tested as pure functions over a parsed+checked document.
