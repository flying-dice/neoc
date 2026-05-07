# `lib:*`: Blessed gap-fillers and project-native modules

The `lib:*` namespace contains modules that do not fit cleanly into either `std:*` (which mirrors the Rust standard library) or `vnd:*` (which exposes single vendored crates). Two kinds of module live here: blessed *gap-fillers* that plug a hole the Rust standard library does not cover (such as `lib:base64`), and *project-native* modules written specifically for neoc (such as `lib:test` and `lib:json`).

A module qualifies for `lib:*` when its purpose is well-defined, its surface is stable, and it is genuinely useful to most scripts. Speculative or experimental functionality belongs elsewhere until it has earned a place here.

## Reference

- [`lib:base64`](/lib/reference/base64) — Base64 encoding and decoding.
- [`lib:json`](/lib/reference/json) — JSON Object and Array document handles, with `null` sentinel.
- [`lib:mime`](/lib/reference/mime) — MIME type detection and extension mapping.
- [`lib:test`](/lib/reference/test) — The Luau test runner and assertion surface.
- [`lib:vcr`](/lib/reference/vcr) — Record-and-replay HTTP fixtures for tests.

## See also

- [The module system](/guides/module-system) — How `require("lib:name")` resolves.
- [`std:*`](/std/) — Standard-library mirrors.
- [`vnd:*`](/vnd/) — Vendored crates.
