# `extern "<host>" { … }` FFI syntax

- **Status:** done
- **Area:** `tree-sitter-neoc` (grammar), `neocc-ast`, `neocc-parser`,
  `neocc-types`, `neocc-codegen-{lua,js,py}`
- **Commits:** grammar `0cc608b` (grammar repo `main`), neocc `ff95209` (merge of
  the staged `wt-extern` work)

## Outcome

neoc's FFI now uses proper Rust-style `extern` syntax instead of the interim
`#[module]`/`#[link]`-attribute-on-`ext` hack. The string is the **host** (the
runtime the externals come from), exactly like `extern "C"`:

```neoc
#[link(name = "lib:base64")]
extern "neoc" {
    fn encode(input: string) -> string;
    fn decode(input: string) -> Result<string, string>;
}
```

- Grammar: `extern_block` (`extern "<host>" { ext_item* }`) + `attribute_named_arg`
  (`name = value`), so the Rust-faithful `#[link(name = "x")]` parses cleanly
  (this **closed `link-attr-keyvalue`**). Plain un-hosted `ext fn` is unchanged.
- AST: `ItemKind::Extern { host, items }`, `AttrArg::Named`; `Item::link_name()`
  reads both `#[link(name = "x")]` and the positional fallback.
- Host → target: `neoc`/`lua`/`luau`→Lua, `node`/`bun`/`deno`→Js,
  `python`/`cpython`→Python. Each backend emits the native import
  (`require` / `import { } from` / `from … import`) only when the host maps to
  the target it's emitting; a mismatched-host block emits nothing (error-on-use
  validation is a follow-up).

425 tests pass, conformance unchanged at 212. This is the syntax the `../neoc`
runtime's `neoc emit-headers` produces (MR !56), verified end-to-end: all 20
runtime headers `neocc check` clean.
