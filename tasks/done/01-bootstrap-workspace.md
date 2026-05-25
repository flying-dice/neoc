# Bootstrap + Cargo workspace skeleton

- **Status:** done
- **Area:** repo root, all crates
- **Commits:** `e5cfa1e` (bootstrap), `0eeba24` (workspace skeleton)

## What was done

- `git init`, `Cargo.toml` virtual workspace (`members = ["crates/*"]`,
  resolver 3, edition 2024, workspace lints: `unsafe_code = deny`,
  `clippy::pedantic`), `rust-toolchain.toml` (stable + wasm32 target),
  `.gitlab-ci.yml` (lint / test / wasm jobs), `.gitignore`.
- `CLAUDE.md` (house rules, the golden rule, `ext`-only-seam), the
  `language-review` skill.
- Ten crates stood up with the dependency graph: `neocc` (bin), `neocc-grammar`,
  `neocc-ast`, `neocc-parser`, `neocc-types`, `neocc-macros`, `neocc-codegen-lua`,
  `neocc-compiler`, `neocc-fmt`, `neocc-lsp`, `neocc-wasm`.
- `cargo build` / clippy / fmt green; pure leaf crates build for wasm32.
