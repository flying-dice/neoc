//! A tracing span whose name and target are known only at run time.
//!
//! `tracing`'s macros cache one static callsite per source location, so a span named after the module function
//! being called (`base64.encode`, at target `lua::lib::base64`) builds its `Metadata` by hand. The metadata of a
//! `(target, name)` pair is interned once and reused: the set is bounded by the number of functions, so this is a
//! fixed one-time cost, not a per-call leak.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use tracing_core::Callsite as _;
use tracing_core::callsite::DefaultCallsite;
use tracing_core::callsite::Identifier;
use tracing_core::field::FieldSet;
use tracing_core::metadata::Kind;
use tracing_core::{Level, Metadata};

/// The callsite whose identifier backs every span's (empty) field set; only its `Identifier` is used.
fn shared_callsite() -> &'static DefaultCallsite {
    static META: Metadata<'static> = Metadata::new(
        "neoc.span",
        "lua",
        Level::INFO,
        Some(file!()),
        Some(line!()),
        Some(module_path!()),
        FieldSet::new(&CALL_SPAN_FIELDS, Identifier(&SHARED)),
        Kind::SPAN,
    );
    static SHARED: DefaultCallsite = DefaultCallsite::new(&META);
    &SHARED
}

type Cache = Mutex<HashMap<(String, String), &'static Metadata<'static>>>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The fields a module call's span ([`dyn_span`]) may record: how the call ended (`outcome`: `ok` or `error`) and,
/// when it raised, what (`error`).
pub const CALL_SPAN_FIELDS: [&str; 2] = ["outcome", "error"];

/// An unentered `INFO` span named `name` at `target`, with nothing recorded yet ([`CALL_SPAN_FIELDS`]).
pub fn dyn_span(target: &str, name: &str) -> tracing::Span {
    let meta = {
        let mut cache = cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *cache.entry((target.to_owned(), name.to_owned())).or_insert_with(|| {
            let name: &'static str = Box::leak(name.to_owned().into_boxed_str());
            let target: &'static str = Box::leak(target.to_owned().into_boxed_str());
            let fields = FieldSet::new(&CALL_SPAN_FIELDS, shared_callsite().metadata().callsite());
            Box::leak(Box::new(Metadata::new(
                name,
                target,
                Level::INFO,
                None,
                None,
                None,
                fields,
                Kind::SPAN,
            )))
        })
    };
    let values = meta.fields().value_set(&[]);
    tracing::Span::new(meta, &values)
}

/// The names of the fields a script's span carries ([`script_span`]): what it was opened with and what was recorded
/// on it (`fields`, one JSON object each time), and how it ended (`outcome`, `error`).
pub const SCRIPT_SPAN_FIELDS: [&str; 3] = ["fields", "outcome", "error"];

/// The callsite whose identifier backs every script span's field set.
fn script_callsite() -> &'static DefaultCallsite {
    static META: Metadata<'static> = Metadata::new(
        "neoc.script_span",
        "lua",
        Level::INFO,
        Some(file!()),
        Some(line!()),
        Some(module_path!()),
        FieldSet::new(&SCRIPT_SPAN_FIELDS, Identifier(&SCRIPT)),
        Kind::SPAN,
    );
    static SCRIPT: DefaultCallsite = DefaultCallsite::new(&META);
    &SCRIPT
}

type LevelCache = Mutex<HashMap<(String, String, Level), &'static Metadata<'static>>>;

fn script_cache() -> &'static LevelCache {
    static CACHE: OnceLock<LevelCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// An unentered span a script opened (`lib:tracing`): named `name` at `target` and `level`, with `fields` (a JSON
/// object) recorded at once. Record more with `span.record("fields", json)`, and how it ended with `outcome` and
/// `error`. The metadata of a `(target, name, level)` is interned once, as [`dyn_span`]'s is; a script names its
/// spans in its source, so the set is bounded by the script.
pub fn script_span(target: &str, name: &str, level: Level, fields: &str) -> tracing::Span {
    let meta = {
        let mut cache = script_cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *cache.entry((target.to_owned(), name.to_owned(), level)).or_insert_with(|| {
            let name: &'static str = Box::leak(name.to_owned().into_boxed_str());
            let target: &'static str = Box::leak(target.to_owned().into_boxed_str());
            let set = FieldSet::new(&SCRIPT_SPAN_FIELDS, script_callsite().metadata().callsite());
            Box::leak(Box::new(Metadata::new(name, target, level, None, None, None, set, Kind::SPAN)))
        })
    };
    let Some(field) = meta.fields().field("fields") else { return tracing::Span::none() };
    let values = [(&field, Some(&fields as &dyn tracing::Value))];
    let values = meta.fields().value_set(&values);
    tracing::Span::new(meta, &values)
}
