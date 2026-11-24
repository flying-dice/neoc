//! MIME type detection: the extension to MIME mapping and filename-based detection, `lib:mime`.
//!
//! A type or extension that is not known is normal absence, so `from_extension`, `detect` and `to_extension` return
//! `nil` for it; the only error is text that is not a MIME type (`invalid_mime`), and the preference map is a typed `{
//! [string]: string }`.

/// Canonical extensions where `mime_guess`'s alphabetical first entry is not what people expect. Checked before
/// `mime_guess::get_mime_extensions`.
const RUNTIME_PREFS: &[(&str, &str)] = &[
    ("application/javascript", ".js"),
    ("application/xml", ".xml"),
    ("image/jpeg", ".jpg"),
    ("image/svg+xml", ".svg"),
    ("text/html", ".html"),
    ("text/javascript", ".js"),
    ("text/xml", ".xml"),
];

#[mlua_ir::module("lib:mime")]
pub mod mime {
    use std::collections::BTreeMap;

    use mlua_ir::LuaError;
    use serde::Serialize;

    use super::RUNTIME_PREFS;

    /// Why a MIME type could not be read.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum MimeError {
        /// The text is not a MIME type (`type/subtype`).
        #[error("{mime_type:?} is not a MIME type: {reason}")]
        InvalidMime {
            /// The text as given.
            mime_type: String,
            /// What is wrong with it.
            reason: String,
        },
    }

    /// The MIME type of a file extension, with or without its leading dot; `nil` when the extension is not known.
    ///
    /// ```luau
    /// assert(mime.from_extension(".json") == "application/json")
    /// assert(mime.from_extension("html") == "text/html")
    /// assert(mime.from_extension(".zzzzz") == nil)
    /// ```
    pub fn from_extension(ext: String) -> Option<String> {
        mime_guess::from_ext(ext.strip_prefix('.').unwrap_or(&ext)).first().map(|m| m.to_string())
    }

    /// A file extension (with its dot) for a MIME type: the one in `prefs` if it has one, else a canonical one
    /// (`.jpg` for `image/jpeg`), else the first the database lists; `nil` when none is known. Text that is not a
    /// MIME type is an error.
    ///
    /// ```luau
    /// assert(assert(mime.to_extension("text/html")) == ".html")
    /// assert(assert(mime.to_extension("text/html", { ["text/html"] = ".xhtml" })) == ".xhtml")
    /// local _, err = mime.to_extension("not-a-mime")
    /// assert(err ~= nil and err.kind == "invalid_mime")
    /// ```
    pub fn to_extension(
        mime_type: String,
        prefs: Option<BTreeMap<String, String>>,
    ) -> Result<Option<String>, MimeError> {
        if let Some(ext) = prefs.and_then(|prefs| prefs.get(&mime_type).cloned()) {
            return Ok(Some(ext));
        }
        if let Some((_, ext)) = RUNTIME_PREFS.iter().find(|(mime, _)| *mime == mime_type) {
            return Ok(Some((*ext).to_owned()));
        }
        let mime: mime_guess::mime::Mime =
            mime_type.parse().map_err(|e: mime_guess::mime::FromStrError| MimeError::InvalidMime {
                mime_type: mime_type.clone(),
                reason: e.to_string(),
            })?;
        Ok(mime_guess::get_mime_extensions(&mime).and_then(|exts| exts.first()).map(|ext| format!(".{ext}")))
    }

    /// The MIME type of a file name or path, from its extension; `nil` when it has none or it is not known.
    ///
    /// ```luau
    /// assert(mime.detect("archive.tar.gz") == "application/gzip")
    /// assert(mime.detect("Makefile") == nil)
    /// ```
    pub fn detect(filename: String) -> Option<String> {
        mime_guess::from_path(&filename).first().map(|m| m.to_string())
    }
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    fn engine() -> Lua {
        let lua = neoc_vm_core::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:mime")"#).eval().unwrap();
        lua.globals().set("mime", m).unwrap();
        lua
    }

    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        engine().load(code).eval().unwrap()
    }

    #[test]
    fn from_extension_with_and_without_the_dot() {
        assert_eq!(eval::<String>(r#"return mime.from_extension(".json")"#), "application/json");
        assert_eq!(eval::<String>(r#"return mime.from_extension("html")"#), "text/html");
        assert_eq!(eval::<String>(r#"return mime.from_extension(".css")"#), "text/css");
        assert_eq!(eval::<String>(r#"return mime.from_extension(".png")"#), "image/png");
    }

    #[test]
    fn from_extension_unknown_is_nil() {
        for ext in [".zzzzz", "", "."] {
            let (value, count): (Option<String>, i64) = eval(&format!(
                r##"return mime.from_extension("{ext}"), select("#", mime.from_extension("{ext}"))"##
            ));
            assert!(value.is_none(), "{ext:?}");
            assert_eq!(count, 1, "absence is a single nil, not (nil, error)");
        }
    }

    #[test]
    fn to_extension_canonical_choices() {
        assert_eq!(eval::<String>(r#"return (mime.to_extension("text/html"))"#), ".html");
        assert_eq!(eval::<String>(r#"return (mime.to_extension("image/jpeg"))"#), ".jpg");
        assert_eq!(eval::<String>(r#"return (mime.to_extension("application/json"))"#), ".json");
        assert_eq!(eval::<String>(r#"return (mime.to_extension("text/css"))"#), ".css");
    }

    #[test]
    fn to_extension_user_prefs_override_then_fall_through() {
        let (overridden, json, jpeg, css): (String, String, String, String) = eval(
            r#"
            local prefs = { ["text/html"] = ".xhtml" }
            return (mime.to_extension("text/html", prefs)), (mime.to_extension("application/json", prefs)),
                   (mime.to_extension("image/jpeg", prefs)), (mime.to_extension("text/css", prefs))
        "#,
        );
        assert_eq!(
            (overridden.as_str(), json.as_str(), jpeg.as_str(), css.as_str()),
            (".xhtml", ".json", ".jpg", ".css")
        );
    }

    #[test]
    fn to_extension_of_a_type_without_one_is_nil() {
        assert!(
            eval::<Option<String>>(r#"return (mime.to_extension("application/x-neoc-nothing"))"#).is_none()
        );
    }

    #[test]
    fn to_extension_of_text_that_is_not_a_mime_type_is_a_typed_error() {
        for bad in ["not-a-mime", "", "."] {
            let (value, kind, op, mime_type, text): (Option<String>, String, String, String, String) = eval(
                &format!(
                    r#"local v, e = mime.to_extension("{bad}") return v, e.kind, e.op, e.mime_type, tostring(e)"#
                ),
            );
            assert!(value.is_none());
            assert_eq!(
                (kind.as_str(), op.as_str(), mime_type.as_str()),
                ("invalid_mime", "lib:mime.to_extension", bad)
            );
            assert!(text.starts_with("lib:mime.to_extension: invalid_mime:"), "{text}");
        }
    }

    #[test]
    fn a_prefs_map_with_a_non_string_value_is_refused() {
        let err = engine()
            .load(r#"return mime.to_extension("text/html", { ["text/html"] = 1 })"#)
            .eval::<mlua::Value>()
            .unwrap_err()
            .to_string();
        assert!(err.contains("lib:mime.to_extension: argument #2 'prefs'"), "{err}");
    }

    #[test]
    fn detect_by_filename() {
        assert_eq!(eval::<String>(r#"return mime.detect("style.css")"#), "text/css");
        assert_eq!(eval::<String>(r#"return mime.detect("archive.tar.gz")"#), "application/gzip");
        assert_eq!(eval::<String>(r#"return mime.detect("assets/images/logo.png")"#), "image/png");
    }

    #[test]
    fn detect_without_a_known_extension_is_nil() {
        for name in ["Makefile", "", "."] {
            assert!(
                eval::<Option<String>>(&format!(r#"return mime.detect("{name}")"#)).is_none(),
                "{name:?}"
            );
        }
    }
}
