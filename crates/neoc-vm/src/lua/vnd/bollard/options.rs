//! What a script passes to a `vnd:bollard` call, read before the call's future exists.
//!
//! A script table can run script code when it is read (a `__index`), so every argument is turned into plain data
//! ([`Json`]) in the synchronous part of the call, as `vnd:hyper` reads its options. Two kinds of table arrive:
//!
//! - **options** ([`Opts`]): this module's own lower-case keys (`all`, `force`, `filters`, `tail`). A key that is not
//!   one of the call's is refused, so a misspelt option fails instead of being ignored.
//! - **Docker's own bodies** ([`body`]): the create calls take Docker's JSON as a table, with Docker's field names
//!   (`Image`, `HostConfig`, `Labels`), and pass it through.
//!
//! Luau has one table for a list and a map, so an empty table says neither. [`tidy`] leaves every empty table out:
//! to Docker an absent field and an empty list or map mean the same in every create body. It also writes a whole
//! number as an integer, which is what Docker's integer fields take.

use std::collections::HashMap;

use mlua::Value;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value as Json};

use super::error::BollardError;
use crate::lua::serde_guard::refuse_buffer;

/// The longest name of a container, volume or network Docker takes, and far more than any image reference needs.
const MAX_NAME: usize = 512;

/// A script value as plain data. A buffer is refused (raised, as every serde conversion of a script value does:
/// `serde_guard`); anything else that cannot be data (a function, a table that holds itself) is `invalid_input`.
pub(super) fn json(value: &Value, op: &str) -> mlua::Result<Result<Json, BollardError>> {
    refuse_buffer(value, op)?;
    Ok(serde_json::to_value(value)
        .map(tidy)
        .map_err(|e| BollardError::invalid_input(e.to_string())))
}

/// `value` without its empty tables and with whole numbers as integers (see the module docs). An empty Luau table
/// arrives as an empty object; an empty array is dropped as well, so that the rule does not depend on that.
fn tidy(value: Json) -> Json {
    match value {
        Json::Array(items) if items.is_empty() => Json::Null,
        Json::Array(items) => Json::Array(items.into_iter().map(tidy).collect()),
        Json::Object(fields) => {
            let kept: Map<String, Json> = fields
                .into_iter()
                .map(|(key, value)| (key, tidy(value)))
                .filter(|(_, value)| !value.is_null())
                .collect();
            if kept.is_empty() {
                Json::Null
            } else {
                Json::Object(kept)
            }
        }
        Json::Number(n) => match n.as_f64() {
            // 2^53: past it a double no longer holds every whole number, so it is left as it was written
            Some(f) if !n.is_i64() && f.fract() == 0.0 && f.abs() <= 9_007_199_254_740_992.0 => {
                Json::from(f as i64)
            }
            _ => Json::Number(n),
        },
        other => other,
    }
}

/// One of Docker's request bodies from the table a script gave: `what` names it in the error.
pub(super) fn body<T: DeserializeOwned>(value: Json, what: &str) -> Result<T, BollardError> {
    if !value.is_object() {
        return Err(BollardError::invalid_input(format!(
            "{what} must be a table with Docker's field names"
        )));
    }
    serde_json::from_value(value).map_err(|e| BollardError::invalid_input(format!("{what}: {e}")))
}

/// The name of a container, volume or network as it goes into the request path: Docker's own alphabet
/// (`[A-Za-z0-9][A-Za-z0-9_.-]*`), so a name cannot reach another endpoint. An id is hexadecimal and passes.
pub(super) fn name(name: &str, what: &str) -> Result<String, BollardError> {
    let first_ok = name
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_alphanumeric());
    let rest_ok = name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'));
    if !first_ok || !rest_ok || name.len() > MAX_NAME {
        return Err(BollardError::invalid_input(format!(
            "{what} must be a name or an id: letters, digits, `_`, `.` and `-`, starting with a letter or digit"
        )));
    }
    Ok(name.to_owned())
}

/// An image reference as it goes into the request path or query: a name's alphabet plus `/`, `:` and `@`
/// (`registry:5000/team/app:1.2`, `app@sha256:...`), with no empty or dot-only path part.
pub(super) fn image(reference: &str) -> Result<String, BollardError> {
    let chars_ok = reference
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'/' | b':' | b'@'));
    let parts_ok = reference
        .split('/')
        .all(|part| !part.is_empty() && part != "." && part != "..");
    if !chars_ok || !parts_ok || reference.len() > MAX_NAME {
        return Err(BollardError::invalid_input(
            "the image must be a reference like `nginx`, `nginx:1.27` or `registry:5000/team/app@sha256:...`",
        ));
    }
    Ok(reference.to_owned())
}

/// An image reference as the repository and the tag a pull names. Without a tag or a digest the tag is `latest`:
/// Docker's API pulls *every* tag of a repository when none is given, which no caller of `pull_image` means.
pub(super) fn pull_target(reference: &str) -> Result<(String, Option<String>), BollardError> {
    let reference = image(reference)?;
    if reference.contains('@') {
        return Ok((reference, None));
    }
    let last_part = reference.rsplit('/').next().unwrap_or(&reference);
    Ok(match last_part.rsplit_once(':') {
        Some((_, tag)) => {
            let repo = &reference[..reference.len() - tag.len() - 1];
            (repo.to_owned(), Some(tag.to_owned()))
        }
        None => (reference, Some("latest".to_owned())),
    })
}

/// The options table of one call.
pub(super) struct Opts {
    fields: Map<String, Json>,
}

impl Opts {
    /// The options a script gave (`nil` and `{}` are no options), which may hold only the keys in `allowed`.
    pub(super) fn read(value: Option<Json>, allowed: &[&str]) -> Result<Self, BollardError> {
        let fields = match value {
            None | Some(Json::Null) => Map::new(),
            Some(Json::Object(fields)) => fields,
            Some(_) => {
                return Err(BollardError::invalid_input(
                    "the options must be a table of named options",
                ));
            }
        };
        if let Some(unknown) = fields.keys().find(|key| !allowed.contains(&key.as_str())) {
            return Err(BollardError::invalid_input(format!(
                "unknown option `{unknown}` (this call takes: {})",
                allowed.join(", ")
            )));
        }
        Ok(Self { fields })
    }

    pub(super) fn bool(&self, key: &str) -> Result<Option<bool>, BollardError> {
        match self.fields.get(key) {
            None => Ok(None),
            Some(Json::Bool(b)) => Ok(Some(*b)),
            Some(_) => Err(wrong(key, "true or false")),
        }
    }

    pub(super) fn string(&self, key: &str) -> Result<Option<String>, BollardError> {
        match self.fields.get(key) {
            None => Ok(None),
            Some(Json::String(s)) => Ok(Some(s.clone())),
            Some(_) => Err(wrong(key, "a string")),
        }
    }

    /// A whole number that fits Docker's 32-bit parameters (a count of lines, seconds, a Unix time).
    pub(super) fn int(&self, key: &str) -> Result<Option<i32>, BollardError> {
        match self.fields.get(key) {
            None => Ok(None),
            Some(value) => value
                .as_i64()
                .and_then(|n| i32::try_from(n).ok())
                .map(Some)
                .ok_or_else(|| wrong(key, "a whole number")),
        }
    }

    /// A positive number of seconds.
    pub(super) fn seconds(&self, key: &str) -> Result<Option<f64>, BollardError> {
        match self.fields.get(key) {
            None => Ok(None),
            Some(value) => value
                .as_f64()
                .filter(|secs| secs.is_finite() && *secs > 0.0)
                .map(Some)
                .ok_or_else(|| wrong(key, "a positive number of seconds")),
        }
    }

    pub(super) fn strings(&self, key: &str) -> Result<Option<Vec<String>>, BollardError> {
        match self.fields.get(key) {
            None => Ok(None),
            Some(value) => string_list(value)
                .map(Some)
                .ok_or_else(|| wrong(key, "a list of strings")),
        }
    }

    /// Docker's `filters`: each name with the values it may have (`{ label = { "app=web" }, status = "running" }`;
    /// one value may be written without its list).
    pub(super) fn filters(&self) -> Result<Option<HashMap<String, Vec<String>>>, BollardError> {
        let expected = "a table of filter names, each with a string or a list of strings";
        match self.fields.get("filters") {
            None => Ok(None),
            Some(Json::Object(filters)) => filters
                .iter()
                .map(|(name, values)| {
                    let values = match values {
                        Json::String(one) => Some(vec![one.clone()]),
                        many => string_list(many),
                    };
                    values.map(|values| (name.clone(), values))
                })
                .collect::<Option<HashMap<_, _>>>()
                .map(Some)
                .ok_or_else(|| wrong("filters", expected)),
            Some(_) => Err(wrong("filters", expected)),
        }
    }
}

fn string_list(value: &Json) -> Option<Vec<String>> {
    value
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(str::to_owned))
        .collect()
}

fn wrong(key: &str, expected: &str) -> BollardError {
    BollardError::invalid_input(format!("option `{key}` must be {expected}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tidy_drops_empty_tables_and_writes_whole_numbers_as_integers() {
        let tidied = tidy(json!({
            "Image": "nginx",
            "Cmd": [],
            "Labels": {},
            "HostConfig": { "Memory": 1024.0, "Binds": [], "NanoCpus": 1.5 },
            "Env": ["A=1"],
        }));
        assert_eq!(
            tidied,
            json!({ "Image": "nginx", "HostConfig": { "Memory": 1024, "NanoCpus": 1.5 }, "Env": ["A=1"] })
        );
        assert_eq!(tidied["HostConfig"]["Memory"].as_i64(), Some(1024));
    }

    #[test]
    fn a_name_is_dockers_alphabet_and_cannot_leave_its_path_segment() {
        for good in ["web", "web-1", "a.b_c", "0123abcdef", "A"] {
            assert_eq!(name(good, "the container").unwrap(), good);
        }
        for bad in [
            "", "-web", ".", "..", "a/b", "a?b", "a b", "a%2fb", "a#b", "é",
        ] {
            assert!(name(bad, "the container").is_err(), "{bad:?} was accepted");
        }
        assert!(name(&"a".repeat(MAX_NAME + 1), "the container").is_err());
    }

    #[test]
    fn an_image_reference_keeps_its_registry_tag_and_digest() {
        for good in [
            "nginx",
            "nginx:1.27",
            "library/nginx",
            "registry:5000/team/app:1.2",
            "app@sha256:abc123",
        ] {
            assert_eq!(image(good).unwrap(), good);
        }
        for bad in [
            "", "/nginx", "nginx/", "a//b", "../etc", "a/./b", "a?b", "a b", "a%b",
        ] {
            assert!(image(bad).is_err(), "{bad:?} was accepted");
        }
    }

    #[test]
    fn a_pull_without_a_tag_names_latest_and_never_every_tag() {
        let target = |reference| pull_target(reference).unwrap();
        assert_eq!(target("nginx"), ("nginx".into(), Some("latest".into())));
        assert_eq!(target("nginx:1.27"), ("nginx".into(), Some("1.27".into())));
        assert_eq!(
            target("registry:5000/team/app"),
            ("registry:5000/team/app".into(), Some("latest".into()))
        );
        assert_eq!(
            target("registry:5000/team/app:1.2"),
            ("registry:5000/team/app".into(), Some("1.2".into()))
        );
        assert_eq!(target("app@sha256:abc"), ("app@sha256:abc".into(), None));
    }

    #[test]
    fn options_refuse_unknown_keys_and_wrong_types() {
        let read = |value| Opts::read(Some(value), &["all", "tail", "filters", "timeout"]);
        assert!(Opts::read(None, &[]).is_ok());
        assert!(read(Json::Null).is_ok());
        let unknown = read(json!({ "al": true })).err().unwrap().to_string();
        assert!(unknown.contains("unknown option `al`"), "{unknown}");
        assert!(read(json!("all")).is_err());

        let opts = read(json!({ "all": true, "tail": 50, "timeout": 2.5 })).unwrap();
        assert_eq!(opts.bool("all").unwrap(), Some(true));
        assert_eq!(opts.int("tail").unwrap(), Some(50));
        assert_eq!(opts.seconds("timeout").unwrap(), Some(2.5));
        assert!(opts.string("all").is_err());
        assert!(opts.bool("tail").is_err());
        assert!(read(json!({ "tail": 1.5 })).unwrap().int("tail").is_err());
        assert!(read(json!({ "tail": 1e12 })).unwrap().int("tail").is_err());
        assert!(
            read(json!({ "timeout": 0 }))
                .unwrap()
                .seconds("timeout")
                .is_err()
        );
    }

    #[test]
    fn filters_take_one_value_or_a_list() {
        let opts = Opts::read(
            Some(json!({ "filters": { "status": "running", "label": ["a=1", "b"] } })),
            &["filters"],
        )
        .unwrap();
        let filters = opts.filters().unwrap().unwrap();
        assert_eq!(filters["status"], vec!["running"]);
        assert_eq!(filters["label"], vec!["a=1", "b"]);
        let bad = Opts::read(Some(json!({ "filters": { "status": 1 } })), &["filters"]).unwrap();
        assert!(bad.filters().is_err());
    }
}
