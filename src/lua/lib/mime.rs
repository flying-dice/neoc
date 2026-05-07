//! MIME type detection — extension ↔ MIME mapping and filename-based detection.
//!
//! See docs/src/lib/reference/mime.md for the public surface contract.

use mlua::{Lua, Table, Value};
use tracing::instrument;

use crate::lua::sandbox::{from_err, ok};

/// Built-in preference map for canonical extensions where `mime_guess`'s
/// alphabetical first entry isn't what users expect. Checked before falling
/// back to `mime_guess::get_mime_extensions`.
static RUNTIME_PREFS: &[(&str, &str)] = &[
    ("application/javascript", ".js"),
    ("application/xml", ".xml"),
    ("image/jpeg", ".jpg"),
    ("image/svg+xml", ".svg"),
    ("text/html", ".html"),
    ("text/javascript", ".js"),
    ("text/xml", ".xml"),
];

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("from_extension", lua.create_function(from_extension)?)?;
    t.set("to_extension", lua.create_function(to_extension)?)?;
    t.set("detect", lua.create_function(detect)?)?;
    Ok(t)
}

#[instrument(name = "mime.from_extension", target = "luau::lib::mime", skip_all)]
fn from_extension(lua: &Lua, ext: String) -> mlua::Result<(Value, Value)> {
    // Normalize: strip leading dot if present, so both ".json" and "json" work.
    let ext_clean = ext.strip_prefix('.').unwrap_or(&ext);
    match mime_guess::from_ext(ext_clean).first() {
        Some(mime) => ok(lua, mime.to_string()),
        None => from_err(
            lua,
            "mime.from_extension",
            format!("unknown extension: {ext}"),
        ),
    }
}

#[instrument(name = "mime.to_extension", target = "luau::lib::mime", skip_all)]
fn to_extension(
    lua: &Lua,
    (mime_type, user_prefs): (String, Option<Table>),
) -> mlua::Result<(Value, Value)> {
    // 1. User preference map (optional Lua table arg).
    if let Some(ref prefs) = user_prefs
        && let Some(ext) = prefs.get::<Option<String>>(mime_type.as_str())?
    {
        return ok(lua, ext);
    }

    // 2. Runtime preference map (canonical extensions).
    for &(mime, ext) in RUNTIME_PREFS {
        if mime == mime_type {
            return ok(lua, ext.to_string());
        }
    }

    // 3. Fall back to mime_guess.
    let mime: mime_guess::mime::Mime = match mime_type.parse() {
        Ok(m) => m,
        Err(e) => return from_err(lua, "mime.to_extension", e),
    };
    match mime_guess::get_mime_extensions(&mime) {
        Some(exts) if !exts.is_empty() => ok(lua, format!(".{}", exts[0])),
        _ => from_err(
            lua,
            "mime.to_extension",
            format!("no extension known for: {mime_type}"),
        ),
    }
}

#[instrument(name = "mime.detect", target = "luau::lib::mime", skip_all)]
fn detect(lua: &Lua, filename: String) -> mlua::Result<(Value, Value)> {
    match mime_guess::from_path(&filename).first() {
        Some(mime) => ok(lua, mime.to_string()),
        None => from_err(
            lua,
            "mime.detect",
            format!("cannot determine MIME type for: {filename}"),
        ),
    }
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use criterion::black_box;

    let lua = crate::lua::sandbox::make_engine().expect("engine");

    let mut g = c.benchmark_group("mime");

    g.bench_function("library", |b| {
        b.iter(|| {
            let _ = mime_guess::from_ext(black_box("json")).first();
        });
    });

    let ident: mlua::Function = lua.create_function(|_, s: String| Ok(s)).expect("ident");
    g.bench_function("into_out_lua", |b| {
        b.iter(|| {
            let _: String = ident.call(black_box("json".to_string())).unwrap();
        });
    });

    let chunk: mlua::Function = lua
        .load(r#"return function(ext) return (require("lib:mime")).from_extension(ext) end"#)
        .eval()
        .expect("chunk");
    g.bench_function("through_lua", |b| {
        b.iter(|| {
            let _: String = chunk.call(black_box("json".to_string())).unwrap();
        });
    });

    g.finish();
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    fn engine() -> Lua {
        let lua = crate::lua::sandbox::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:mime")"#).eval().unwrap();
        lua.globals().set("mime", m).unwrap();
        lua
    }

    #[test]
    fn from_extension_json() {
        let lua = engine();
        let (val, err): (String, Option<String>) = lua
            .load(r#"return mime.from_extension(".json")"#)
            .eval()
            .unwrap();
        assert_eq!(val, "application/json");
        assert!(err.is_none());
    }

    #[test]
    fn from_extension_without_dot() {
        let lua = engine();
        let (val, err): (String, Option<String>) = lua
            .load(r#"return mime.from_extension("html")"#)
            .eval()
            .unwrap();
        assert_eq!(val, "text/html");
        assert!(err.is_none());
    }

    #[test]
    fn from_extension_unknown() {
        let lua = engine();
        let (val, err): (Option<String>, Option<String>) = lua
            .load(r#"return mime.from_extension(".zzzzz")"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        assert!(err.unwrap().starts_with("mime.from_extension:"));
    }

    #[test]
    fn to_extension_text_html() {
        let lua = engine();
        let (val, err): (String, Option<String>) = lua
            .load(r#"return mime.to_extension("text/html")"#)
            .eval()
            .unwrap();
        assert_eq!(val, ".html");
        assert!(err.is_none());
    }

    #[test]
    fn to_extension_image_jpeg() {
        let lua = engine();
        let (val, err): (String, Option<String>) = lua
            .load(r#"return mime.to_extension("image/jpeg")"#)
            .eval()
            .unwrap();
        assert_eq!(val, ".jpg");
        assert!(err.is_none());
    }

    #[test]
    fn to_extension_user_prefs_override() {
        let lua = engine();
        let (val, err): (String, Option<String>) = lua
            .load(
                r#"
                local prefs = { ["text/html"] = ".xhtml" }
                return mime.to_extension("text/html", prefs)
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(val, ".xhtml");
        assert!(err.is_none());
    }

    #[test]
    fn to_extension_user_prefs_fallthrough() {
        let lua = engine();
        // User prefs don't have application/json, so it falls through to
        // runtime prefs / mime_guess.
        let (val, err): (String, Option<String>) = lua
            .load(
                r#"
                local prefs = { ["text/html"] = ".xhtml" }
                return mime.to_extension("application/json", prefs)
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(val, ".json");
        assert!(err.is_none());
    }

    #[test]
    fn to_extension_invalid_mime() {
        let lua = engine();
        let (val, err): (Option<String>, Option<String>) = lua
            .load(r#"return mime.to_extension("not-a-mime")"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        assert!(err.unwrap().starts_with("mime.to_extension:"));
    }

    #[test]
    fn detect_filename() {
        let lua = engine();
        let (val, err): (String, Option<String>) = lua
            .load(r#"return mime.detect("archive.tar.gz")"#)
            .eval()
            .unwrap();
        assert_eq!(val, "application/gzip");
        assert!(err.is_none());
    }

    #[test]
    fn detect_no_extension() {
        let lua = engine();
        let (val, err): (Option<String>, Option<String>) = lua
            .load(r#"return mime.detect("Makefile")"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        assert!(err.unwrap().starts_with("mime.detect:"));
    }
}
