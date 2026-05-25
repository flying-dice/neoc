# Parser — CST → AST adapter (`neocc-parser`)

- **Status:** done
- **Area:** `crates/neocc-parser`
- **Commit:** `2ddb95b`

## What was done

A modular CST→AST adapter (`item.rs`, `ty.rs`, `expr.rs`, `pattern.rs`) that walks
the tree-sitter tree and builds `neocc-ast`. Structural and type-independent —
every expression and pattern kind is handled here. Unsupported nodes push a
diagnostic instead of panicking.

`parse(src) -> ParseResult { module, diagnostics, ok() }`. A trailing
semicolon-less block-like expression is promoted to the block's value (Rust's
tail-expression rule). Five AST-stage tests assert the produced AST directly, so
an emitter bug can't hide behind them. A regression test covers the fix where
qualified unit-variant patterns (`Color::Red`) were lowering to an empty path.
