# Formatter — `neocc fmt`

- **Status:** done
- **Area:** `crates/neocc-fmt`, `crates/neocc` (`fmt` subcommand)
- **Commits:** `7d97ed0` (formatter), `d4d8ac4` (CLI wiring), `+` rustfmt-alignment
  (4-space indent, expanded struct/enum decls) and corpus reformat

## Outcome

`neocc fmt` is a **CST-based** formatter (not AST) so comments survive. It
parses with `neocc_grammar::LANGUAGE` and walks the concrete tree with a
structured pretty-printer (`format(src) -> Result<String, FmtError>`):
idempotent, `Err` on a parse error. Covers the whole grammar surface (items,
statements, the `cond_*` expression family, patterns, types, attributes,
generics/`where`); leading comments hug the item below, trailing same-line
comments stay.

Canonical style **mirrors rustfmt** — neoc is Rust-shaped, so its formatted
form looks like its Rust counterpart: 4-space indent, struct field lists and
enum variants expanded one per line with a trailing comma (struct *literals*
stay inline), match arms one per line. The 20 `lang/CONFORMANCE/dynamic/*.neoc`
were reformatted to this canonical form (they now visually match the paired
`.rs` anchors).

CLI: `neocc fmt PATHS` formats in place; `--check` writes nothing and exits
non-zero on unformatted files. Tests: messy→canonical (incl. comments + doc
comments), idempotency, and a corpus test asserting `fmt` is a no-op on the
corpus.

---

(original spec follows)

- **Depends on:** `neocc-ast`, `neocc-parser`

## Goal

Parse `.neoc` to AST and pretty-print canonical neoc. A formatter ideally
round-trips through the CST to preserve comments (the AST is span-free and drops
ordinary comments today) — decide whether to format from the AST (loses
comments) or walk the tree-sitter tree directly (preserves them).

## Acceptance

- [ ] `neocc fmt <file>` rewrites a file to canonical form; idempotent
      (formatting twice is a no-op).
- [ ] Tests: messy input → expected canonical output; format(format(x)) == format(x).

## Notes

- Comment preservation is the main design call. Tree-sitter retains comments in
  the CST; the typed AST does not. A CST-based formatter keeps them.
