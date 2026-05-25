# Language spec — the `lang/` reference manual

- **Status:** done (one gap: `SYNTAX.md` §10 — see `tasks/conformance-breadth.md`)
- **Area:** `lang/`
- **Commits:** `bbd7900` (rationale), `d11b4a5` (scaffold), `e811861` (4 layers)

## What was done

The language defined from first principles as "Rust without lifetimes",
**not** transcribed from the bun-oas TypeScript dialect:

- `RATIONALE.md`, `NOTATION.md` (EBNF dialect, inference-rule shape, lowering
  brackets).
- `LEXICAL.md` (tokens, keywords, literals, comments), `SYNTAX.md` (EBNF grammar),
  `STATIC.md` (name resolution, type system, inference rules, `?` rule, match
  exhaustiveness, trait-impl completeness), `DYNAMIC.md` (lowering table to Lua 5.1).
- `CONFORMANCE/` layout.

Deliberately Rust-shaped: `enum` (not `A|B` unions), struct literals (not
`.new({…})`), `if`/`match` expressions (not ternary), closures `|x| …` (not
arrow fns), `use`/`::` (not `import`/`from`), `+` for concat (not Lua `..`), no
`null`. The golden rule (no host-language leaks) governs all of it.
