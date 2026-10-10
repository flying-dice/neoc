# Architecture Decision Records

ADRs capture significant decisions shaping `neoc`. Short, dated records of decisions and their motivating forces. Format: [npryce/adr-tools](https://github.com/npryce/adr-tools).

ADRs are *retrospective* (decisions in force). [RFCs](/rfcs/) are *forward-looking* (proposals not yet accepted). Accepted RFCs producing load-bearing decisions get a corresponding ADR.

## Status

| Status | Meaning |
|---|---|
| `Proposed` | Under discussion. Not in force. |
| `Accepted` | In force. Codebase must comply. |
| `Deprecated` | No longer recommended. No replacement yet. |
| `Superseded` | Replaced by later ADR (linked). |

## Index

- [0001. Use Luau as the embedded scripting language](/adrs/0001-use-luau-as-the-embedded-language) — superseded by 0011
- [0002. Strip unsafe Lua standard library](/adrs/0002-strip-unsafe-lua-stdlib)
- [0003. Three-namespace module structure (`std`, `lib`, `vnd`)](/adrs/0003-three-namespace-module-structure)
- [0004. `vnd:*` names match upstream crate names verbatim](/adrs/0004-vnd-module-names-match-upstream)
- [0005. Tuple-return convention for recoverable failures](/adrs/0005-tuple-error-convention)
- [0006. Hidden async with synchronous-looking Lua APIs](/adrs/0006-hidden-async-with-sync-looking-apis)
- [0007. `std:process` — env, args, child-process spawning](/adrs/0007-std-process-module)
- [0008. `lib:http` — Static file server with directory index](/adrs/0008-lib-http-static-file-server)
- [0009. `--eval` / `-e` CLI flag](/adrs/0009-eval-cli-flag)
- [0010. Independent codegen backends — no shared crate](/adrs/0010-independent-codegen-backends)
- [0011. Use LuaJIT as the embedded runtime and sole compile target](/adrs/0011-use-luajit-as-the-embedded-runtime)

## See also

- [Template](/adrs/template)
- [RFCs](/rfcs/)
- [The module system](/guides/module-system) — touchpoint for ADRs 0003, 0004, 0005
