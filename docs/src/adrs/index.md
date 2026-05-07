# Architecture Decision Records

Architecture Decision Records (ADRs) capture significant decisions that shape `neoc`. Each ADR is a short, dated record of a decision and the forces that motivated it. The format follows [npryce/adr-tools](https://github.com/npryce/adr-tools).

ADRs are *retrospective*: they document decisions that have been made and are in force. By contrast, [RFCs](/rfcs/) are *forward-looking*: they propose changes that have not yet been accepted. An accepted RFC that introduces a load-bearing decision typically produces a corresponding ADR; see the relevant RFC for the design rationale, and the ADR for the resulting decision.

## Status

Each ADR carries one of the following statuses in its front matter and the body:

| Status | Meaning |
|---|---|
| `Proposed` | The decision is under discussion. Not yet in force. |
| `Accepted` | The decision is in force. The codebase is expected to comply. |
| `Deprecated` | The decision is no longer recommended. A replacement has not yet superseded it. |
| `Superseded` | The decision has been replaced by a later ADR, which is linked from the body. |

## Index

- [0001. Use Luau as the embedded scripting language](/adrs/0001-use-luau-as-the-embedded-language)
- [0002. Strip the unsafe halves of the Lua standard library](/adrs/0002-strip-unsafe-lua-stdlib)
- [0003. Three-namespace module structure (`std`, `lib`, `vnd`)](/adrs/0003-three-namespace-module-structure)
- [0004. `vnd:*` module names match upstream crate names verbatim](/adrs/0004-vnd-module-names-match-upstream)
- [0005. Tuple-return convention for recoverable failures](/adrs/0005-tuple-error-convention)
- [0006. Hidden async with synchronous-looking Lua APIs](/adrs/0006-hidden-async-with-sync-looking-apis)
- 0007 — withdrawn before acceptance; slot reserved, not available for reuse.
- [0008. `std:process` — environment, arguments, and child-process spawning](/adrs/0008-std-process-module)
- [0008. `lib:http` — Static file server with directory index](/adrs/0008-lib-http-static-file-server)
- [0009. `--eval` / `-e` CLI flag for single-shot script execution](/adrs/0009-eval-cli-flag)

## See also

- [Template](/adrs/template) — Copy when creating a new ADR.
- [RFCs](/rfcs/) — Forward-looking design proposals.
- [The module system](/guides/module-system) — Touchpoint for ADRs 0003, 0004, and 0005.
