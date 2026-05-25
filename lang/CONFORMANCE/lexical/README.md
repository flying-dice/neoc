# Lexical conformance — deferred

This layer is empty on purpose. The `.tokens` format in the parent README
(`KIND@line:col "lexeme"`, one token per line) needs a tokeniser that emits the
`LEXICAL.md` §8 token classes (`PLUS`, `ARROW`, `INT_LITERAL`, …) as a flat
stream. neoc has no such API.

The only parser is `neoc-grammar` (tree-sitter). Tree-sitter produces a concrete
syntax tree, not a token stream: its terminals for keywords and punctuation are
anonymous nodes, and it carries no mapping from those anonymous tokens to the
named `LEXICAL.md` token classes. Reconstructing `KIND@line:col "lexeme"` from
the CST would mean hand-writing that mapping in the runner — fabricating a token
API the spec doesn't have. The parent README is explicit that expected outputs
must be hand-written against the spec, never derived from whatever the impl
happens to produce; a synthesised mapping would violate that.

So this layer waits for a real tokeniser. When neoc grows a standalone lexer
(`neocc_lexer::tokenize(&str) -> Vec<Token>` with `LEXICAL.md`-named kinds),
add cases here as `<name>.neoc` + `<name>.tokens` and a runner alongside
`conformance_static.rs` / `conformance_syntax.rs`.

Until then the lexical contract is exercised indirectly: every `syntax/` and
`dynamic/` case tokenises as a side effect of parsing, so a lexer regression
that changes the token stream surfaces as a parse or runtime failure there.
