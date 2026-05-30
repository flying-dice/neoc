//! Source Map v3 sidecars: a `transpile_project` of a module whose target has a
//! source map emits an `x.<ext>.map` beside the module and appends a
//! `sourceMappingURL` trailer to the emitted source.
//!
//! The map is line-granular (`lang/` source-map seam, RFC #56): each generated
//! line maps to its neoc source line at column 0. Lua carries a map today; the
//! JS/TS backends do not yet, so they emit no `.map` and no trailer — asserted
//! here so the contract is pinned either way.

use std::fs;
use std::path::{Path, PathBuf};

use neoc_compiler::{Target, transpile_project};

/// A unique temp directory for one test, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let mut p = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        p.push(format!("neoc-source-maps-{tag}-{nanos}"));
        fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Lays out a single-module project at `root` for `target` and returns the
/// entry path.
fn scaffold(root: &Path, target: Target, body: &str) -> PathBuf {
    fs::write(
        root.join("neoc.toml"),
        format!("[package]\nname = \"demo\"\ntarget = \"{target}\"\n"),
    )
    .unwrap();
    let src = root.join("src").join("main").join("neoc");
    fs::create_dir_all(&src).unwrap();
    let entry = src.join("main.neoc");
    fs::write(&entry, body).unwrap();
    entry
}

/// Decodes one base64-VLQ digit (the inverse of the encoder under test).
fn b64_value(c: char) -> i64 {
    const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    i64::try_from(ALPHABET.find(c).expect("valid base64 digit")).expect("digit fits i64")
}

/// Decodes a base64-VLQ `mappings` string into per-line segments, each segment a
/// vector of decoded fields. Enough to assert the structure round-trips.
fn decode_mappings(mappings: &str) -> Vec<Vec<Vec<i64>>> {
    mappings
        .split(';')
        .map(|line| {
            line.split(',')
                .filter(|s| !s.is_empty())
                .map(decode_segment)
                .collect()
        })
        .collect()
}

/// Decodes one comma-free VLQ segment into its integer fields.
fn decode_segment(seg: &str) -> Vec<i64> {
    let mut fields = Vec::new();
    let mut shift = 0u32;
    let mut acc: i64 = 0;
    for c in seg.chars() {
        let d = b64_value(c);
        let cont = d & 0b10_0000;
        acc += (d & 0b1_1111) << shift;
        if cont == 0 {
            // Sign in bit 0.
            let value = if acc & 1 == 1 { -(acc >> 1) } else { acc >> 1 };
            fields.push(value);
            acc = 0;
            shift = 0;
        } else {
            shift += 5;
        }
    }
    fields
}

/// Finds the `aux_files` entry whose path ends with `suffix`.
fn aux_with_suffix<'a>(
    aux: &'a [(PathBuf, String)],
    suffix: &str,
) -> Option<&'a (PathBuf, String)> {
    aux.iter()
        .find(|(p, _)| p.to_string_lossy().ends_with(suffix))
}

#[test]
fn lua_module_emits_v3_map_sidecar_and_url_trailer() {
    let tmp = TempDir::new("lua-map");
    let body = "pub fn main() {\n    let x = 1;\n    let y = 2;\n}\n";
    let entry = scaffold(tmp.path(), Target::Lua, body);

    let out = transpile_project(&entry, Target::Lua).unwrap();
    assert!(out.ok(), "diagnostics: {:?}", out.diagnostics);

    // The emitted `main.lua` ends in a `--#` sourceMappingURL trailer.
    let (_, lua) = out
        .files
        .iter()
        .find(|(p, _)| p.extension().is_some_and(|e| e == "lua"))
        .expect("a .lua module");
    assert!(
        lua.trim_end()
            .ends_with("--# sourceMappingURL=main.lua.map"),
        "expected a Lua sourceMappingURL trailer, got tail:\n{}",
        &lua[lua.len().saturating_sub(80)..]
    );

    // The `.map` aux file is valid v3 JSON with the right scalar fields and a
    // non-empty `mappings`.
    let (map_path, json) = aux_with_suffix(&out.aux_files, "main.lua.map").expect("a .map sidecar");
    assert!(
        map_path.to_string_lossy().ends_with("main.lua.map"),
        "{map_path:?}"
    );
    assert!(json.contains("\"version\":3"), "{json}");
    assert!(json.contains("\"file\":\"main.lua\""), "{json}");
    assert!(json.contains("\"sources\":[\"main.neoc\"]"), "{json}");
    // `sourcesContent` inlines the neoc source verbatim (newlines escaped).
    assert!(json.contains("\"sourcesContent\":[\""), "{json}");
    assert!(json.contains("pub fn main"), "{json}");
    assert!(json.contains("\"names\":[]"), "{json}");

    // `mappings` is non-empty and decodes to per-line segments. Every segment is
    // a four-field segment (genCol, srcIdx, srcLine, srcCol); the first names a
    // real source line (>= 0) at column 0.
    let mappings = extract_mappings(json);
    assert!(!mappings.is_empty(), "empty mappings in {json}");
    let lines = decode_mappings(&mappings);
    let first = lines.iter().flatten().next().expect("at least one segment");
    assert_eq!(first.len(), 4, "v3 segments have four fields: {first:?}");
    assert_eq!(first[0], 0, "generated column resets to 0 per line");
    assert_eq!(first[1], 0, "single source, index 0");
    assert!(first[2] >= 0, "a real, zero-based source line");
    assert_eq!(first[3], 0, "source column 0");
}

#[test]
fn js_module_without_map_emits_no_sidecar_or_trailer() {
    // The JS backend has no source map yet (RFC #56): with an empty `SourceMap`
    // the driver emits no `.map` and no `//#` trailer. Pins the skip-cleanly
    // contract until the JS backend grows a map.
    let tmp = TempDir::new("js-no-map");
    let entry = scaffold(
        tmp.path(),
        Target::Js,
        "pub fn main() {\n    let x = 1;\n}\n",
    );

    let out = transpile_project(&entry, Target::Js).unwrap();
    assert!(out.ok(), "diagnostics: {:?}", out.diagnostics);

    let (_, js) = out
        .files
        .iter()
        .find(|(p, _)| p.extension().is_some_and(|e| e == "js"))
        .expect("a .js module");
    assert!(
        !js.contains("sourceMappingURL"),
        "JS has no map yet, so no trailer:\n{js}"
    );
    assert!(
        aux_with_suffix(&out.aux_files, ".js.map").is_none(),
        "JS has no map yet, so no .map sidecar"
    );
}

/// Pulls the raw `mappings` string out of the v3 JSON (the value of the last
/// field). Test-local; avoids a JSON dependency.
fn extract_mappings(json: &str) -> String {
    let key = "\"mappings\":\"";
    let start = json.find(key).expect("a mappings field") + key.len();
    let rest = &json[start..];
    let end = rest.find('"').expect("a closing quote");
    rest[..end].to_string()
}
