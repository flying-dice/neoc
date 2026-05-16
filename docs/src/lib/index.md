# `lib:*`: Blessed gap-fillers and project-native modules

Modules that don't fit `std:*` (Rust std mirrors) or `vnd:*` (single vendored crates). Two kinds: *gap-fillers* (`lib:base64`) and *project-native* modules (`lib:test`, `lib:json`).

Qualifies when: well-defined purpose, stable surface, genuinely useful to most scripts.

## Reference

- [`lib:base64`](/lib/reference/base64) — Base64 encoding/decoding
- [`lib:json`](/lib/reference/json) — JSON Object/Array handles, `null` sentinel
- [`lib:mime`](/lib/reference/mime) — MIME type detection and extension mapping
- [`lib:test`](/lib/reference/test) — Luau test runner and assertions
- [`lib:vcr`](/lib/reference/vcr) — Record-and-replay HTTP fixtures

## See also

- [The module system](/guides/module-system)
- [`std:*`](/std/)
- [`vnd:*`](/vnd/)
