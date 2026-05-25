# tasks/done/

Completed work items, one file per milestone, for the record. Each is `done`;
verify against `git log` if a detail looks stale. Open items live one level up
in `tasks/`. The one-screen overview is `tasks/done.md`.

| File | Item | Key commit(s) |
| --- | --- | --- |
| `01-bootstrap-workspace.md` | repo bootstrap + Cargo workspace skeleton | `e5cfa1e`, `0eeba24` |
| `02-language-spec.md` | the `lang/` reference manual (rationale + 4 layers) | `bbd7900`, `e811861` |
| `03-grammar.md` | tree-sitter grammar, `tree-sitter-neoc` repo, submodule | `0eeba24`, `c52aac3` |
| `04-ast.md` | typed AST | `2ddb95b` |
| `05-parser-adapter.md` | CST→AST adapter | `2ddb95b` |
| `06-emitter-core.md` | core Lua emitter + `transpile` pipeline | `40b926d` |
| `07-type-checker.md` | `neocc-types`: check + elaborate + derived-method awareness | `5d6fefa`, `71489aa`, `92b33d8` |
| `08-enum-match-derives.md` | enum/match/patterns, `?`, method dispatch, derives | `d85948e`, `591df87` |
| `09-conformance-and-bugfixes.md` | conformance corpus + the 2 bugs it caught | `4612cbe`, `343295d`, `4d611d8` |
| `10-gitlab-and-zed.md` | three GitLab repos + the Zed extension | (pushed to `flying-dice`) |
| `11-emitter-limitations.md` | `continue` / `let`-destructure / `..base` / enum methods | `3c42e68` |
| `12-cli.md` | `neocc build` / `check` (+ `fmt`/`lsp` dispatch) | `7ce6f2c` |
| `13-newtype-construction.md` | `Wrapper(42)`→`.new`, `.0`→`[1]` | `f3b0d99` |
| `14-match-exhaustiveness-struct.md` | struct `match` exhaustive via irrefutable pattern | `f795b00` |
| `15-lsp.md` | `neocc lsp` server: diagnostics over stdio | `61eb8df`, `96102eb` |
| `16-formatter.md` | `neocc fmt`: CST formatter, rustfmt-aligned style | `7d97ed0`, `d4d8ac4` |
| `17-lsp-features.md` | LSP outline / goto / hover / completion | `8558194`, `ae60f5a`, `3041380` |
| `18-multi-target.md` | Lua + JS + Python backends; corpus 30 cases | `d72e2c5`, `0c88749`, `13d6577`, `1a7df12` |
| `19-extern-ffi.md` | `extern "<host>" { }` + `#[link(name=…)]` FFI syntax | `0cc608b` (grammar), `ff95209` |
| `20-link-attr-keyvalue.md` | key-value attr args (subsumed by extern FFI) | `ff95209` |
| `21-lsp-spans.md` | AST spans → precise type/emit diagnostic ranges | `c0ae4dc`, `a4e06b8`, `047414b` |
| `22-modules-visibility.md` | multi-file `use` resolution + `pub` enforcement | `6178a3b`, `96083f8`, `5e79c29` |
| `23-conformance-breadth.md` | static + syntax corpora + runners; SYNTAX §10 EBNF | `3392870`, `2bad0b0`, `d880d4e` |
| `24-attribute-wf.md` | checker-stage attribute well-formedness | `44ee780`, `acb1d6c` |
