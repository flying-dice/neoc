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
        FieldSet::new(&[], Identifier(&SHARED)),
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

/// An unentered `INFO` span named `name` at `target`, with no fields.
pub(crate) fn dyn_span(target: &str, name: &str) -> tracing::Span {
    let meta = {
        let mut cache = cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *cache.entry((target.to_owned(), name.to_owned())).or_insert_with(|| {
            let name: &'static str = Box::leak(name.to_owned().into_boxed_str());
            let target: &'static str = Box::leak(target.to_owned().into_boxed_str());
            let fields = FieldSet::new(&[], shared_callsite().metadata().callsite());
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
