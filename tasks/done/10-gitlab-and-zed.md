# GitLab repos + the Zed extension

- **Status:** done (Zed install unverified — see notes)
- **Area:** ecosystem (`flying-dice` on `gitlab.beluga-sirius.ts.net`)

## What was done

Three private repos under `flying-dice`:

- **`neocc`** — the compiler workspace.
- **`tree-sitter-neoc`** — the canonical grammar (consumed by `neocc` as a
  submodule; relative URL so the CI job token can clone it).
- **`neoc-zed`** — the Zed editor extension: `extension.toml` (grammar →
  `tree-sitter-neoc@<commit>`, LSP → `neocc lsp`), hand-written highlight queries
  validated against the grammar's node kinds, config/indents/injections/outline.

## Open follow-ups (not blocking)

- The Zed extension is **unverified in a real install** — Zed clones the private
  grammar repo at install time; confirm credentials work over its fetch
  (`zed --install-dev-extension .`), and that Zed accepts the `commit` key.
- `neocc lsp` is a stub, so the extension only does highlighting/outline until
  `tasks/lsp.md` lands.
- GitHub mirroring deferred (private GitLab is the home for now).
