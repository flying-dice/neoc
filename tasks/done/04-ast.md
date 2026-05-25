# Typed AST — `neocc-ast`

- **Status:** done
- **Area:** `crates/neocc-ast`
- **Commit:** `2ddb95b`

## What was done

The full typed AST for the language as pure data: `Module`, `Item`/`ItemKind`,
the declarations (`Fn`, `Struct`, `Enum`, `Trait`, `Impl`, `Ext`, `Use`, `Const`,
`TypeAlias`), `Type`, `Expr` (literals, paths, struct/list literals, calls,
method calls, field/index, binary/unary, range, `if`/`match`/`while`/`for`/`loop`,
closures, block, `?`, return/break/continue), `Pattern`, `Stmt`, `Attribute`.

Span-free, deriving `Debug/Clone/PartialEq`, so the parser can be tested by
asserting the produced AST against an expected value. `BinOp::Concat` is an
internal variant produced only by the type checker's elaboration. No host or
runtime types leak in.

Span-free is a deliberate trade-off; threading spans is deferred until the LSP
needs precise ranges (`tasks/lsp.md`).
