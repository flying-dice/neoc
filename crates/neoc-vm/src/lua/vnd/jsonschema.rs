//! JSON Schema validation — `vnd:jsonschema`.
//!
//! ```lua
//! local jsonschema = require("vnd:jsonschema")
//! local json       = require("vnd:serde_json")
//!
//! local USER_SCHEMA = jsonschema.compile([[
//!   { "type": "object",
//!     "required": ["name", "email"],
//!     "properties": {
//!       "name":  { "type": "string", "maxLength": 255 },
//!       "email": { "type": "string", "maxLength": 255 }
//!     }
//!   }
//! ]])
//!
//! local body = json.from_str(req.body)
//! if not USER_SCHEMA:is_valid(body) then
//!     return { status = 400, body = '{"error":"bad request"}' }
//! end
//! ```
//!
//! ## API
//!
//! - `compile(json_string)` → `Schema` userdata. Parses + compiles the
//!   schema once at module load.
//! - `schema:is_valid(value)` → `bool`. Fast path: shortcut on first
//!   error, no allocation. Use this for handler gates.
//! - `schema:validate(value)` → `ValidationResult` userdata. Walks all
//!   errors, builds a Lua table of error message strings. Use when you
//!   want to surface the failure reasons.
//! - `result:ok()` → `bool`.
//! - `result:errors()` → Lua table of strings (1-indexed). Empty when ok.
//!
//! A value with no JSON encoding (a function, userdata) makes `is_valid`
//! raise, while `validate` returns a result with `ok() == false` and an
//! `invalid input: …` error.
//!
//! `value` may be any Lua value — table, string, number, bool, nil — and
//! is converted to `serde_json::Value` via `serde_json::to_value` before validating.

use std::sync::Arc;

use mlua::{Lua, UserData, UserDataMethods, Value};
use serde_json::Value as JV;

use crate::lua::facade::Module;
use crate::lua::sandbox::{from_err, ok};

/// Compiled schema, shareable across coroutines via `Arc`.
pub struct JSchema {
    inner: Arc<jsonschema::Validator>,
}

/// Bridge a Lua `Value` to a `serde_json::Value` for schema validation.
/// Both `is_valid` and `validate` go through this — they only differ in
/// how they surface the conversion failure.
fn lua_value_to_jv(value: &Value) -> mlua::Result<JV> {
    serde_json::to_value(value).map_err(mlua::Error::external)
}

impl UserData for JSchema {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("is_valid", |_, this, value: Value| {
            // Surface a conversion failure rather than masquerading it as
            // "invalid" — a non-encodable input is a caller bug, not a schema miss.
            let jv = lua_value_to_jv(&value)?;
            Ok(this.inner.is_valid(&jv))
        });

        methods.add_method("validate", |_, this, value: Value| {
            let jv = match lua_value_to_jv(&value) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(JValidationResult {
                        valid: false,
                        errors: vec![format!("invalid input: {}", e)],
                    });
                }
            };
            // iter_errors walks the entire instance — for a "fast gate"
            // the caller should use is_valid instead.
            let errs: Vec<String> = this.inner.iter_errors(&jv).map(|e| e.to_string()).collect();
            Ok(JValidationResult {
                valid: errs.is_empty(),
                errors: errs,
            })
        });
    }
}

pub struct JValidationResult {
    valid: bool,
    errors: Vec<String>,
}

impl UserData for JValidationResult {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("ok", |_, this, _: ()| Ok(this.valid));
        methods.add_method("errors", |lua, this, _: ()| {
            let t = lua.create_table_with_capacity(this.errors.len(), 0)?;
            for (i, e) in this.errors.iter().enumerate() {
                t.set(i + 1, e.as_str())?;
            }
            Ok(t)
        });

        // `__tostring` for display only — `print(result)` and string coercion.
        // It does not affect truthiness: a userdata is always truthy in Lua, so
        // `if result then` is true even for an invalid result. Use `:ok()`.
        methods.add_meta_method(mlua::MetaMethod::ToString, |_, this, _: ()| {
            if this.valid {
                Ok("ValidationResult(ok)".to_string())
            } else {
                Ok(format!("ValidationResult({} errors)", this.errors.len()))
            }
        });
    }
}

pub fn module(lua: &Lua) -> mlua::Result<mlua::Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `vnd:jsonschema` surface through the facade.
///
/// `compile` is set verbatim with [`crate::lua::facade::Module::set`] rather
/// than `try_func` because the hand-written function carries **no**
/// `#[instrument]` span and bakes its own `jsonschema.compile` error prefix —
/// routing through `try_func` would add a span and double the prefix. Its
/// signature is recorded via [`crate::lua::facade::Module::record_fn`]. The
/// `Schema` (`JSchema`) and `ValidationResult` (`JValidationResult`) userdata
/// types keep their own `impl UserData` (methods unchanged) and are recorded
/// via [`crate::lua::facade::Module::userdata`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "vnd:jsonschema")?;

    m.set(
        "compile",
        lua.create_function(|lua, schema_json: mlua::LuaString| {
            let bytes = schema_json.as_bytes();
            let schema_value: JV = match serde_json::from_slice(&bytes) {
                Ok(v) => v,
                Err(e) => return from_err(lua, "jsonschema.compile", e),
            };
            match jsonschema::validator_for(&schema_value) {
                Ok(validator) => ok(
                    lua,
                    JSchema {
                        inner: Arc::new(validator),
                    },
                ),
                Err(e) => from_err(lua, "jsonschema.compile", e),
            }
        })?,
    )?;
    m.record_fn(
        "compile",
        &[("schema_json", "string")],
        "Result<Schema, string>",
        "Parse and compile a JSON Schema string into a `Schema` validator.",
    );

    m.userdata("Schema", |s| {
        s.method(
            "is_valid",
            &[("value", "LuaValue")],
            "bool",
            "Fast validation gate: whether `value` conforms to the schema, short-circuiting on the first error.",
        )
        .method(
            "validate",
            &[("value", "LuaValue")],
            "ValidationResult",
            "Full validation run, returning a `ValidationResult` exposing every error message.",
        );
    });

    m.userdata("ValidationResult", |r| {
        r.method("ok", &[], "bool", "Whether the validated value passed.")
            .method(
                "errors",
                &[],
                "LuaValue",
                "All validation error messages as a 1-indexed table of strings; empty when ok.",
            );
    });

    Ok(m)
}

// ---------------------------------------------------------------------------
// Benchmarks
//
// Three tiers exercising the validate hot path on a typical "user object"
// schema. The interesting deltas:
//   - through_lua - into_out_lua → cost of `serde_json::to_value` + `Validator::is_valid`
//   - into_out_lua - 0           → cost of the mlua bridge alone
//   - library                    → cost of pure jsonschema crate
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use criterion::Throughput;
    use std::hint::black_box;

    const SCHEMA_JSON: &str = r#"{
        "type": "object",
        "required": ["id", "name", "email"],
        "properties": {
            "id":     { "type": "integer" },
            "name":   { "type": "string", "maxLength": 255 },
            "email":  { "type": "string", "maxLength": 255 },
            "active": { "type": "boolean" }
        }
    }"#;
    const INSTANCE_JSON: &str =
        r#"{"id":1,"name":"Alice","email":"alice@example.com","active":true}"#;

    let schema_value: JV = serde_json::from_str(SCHEMA_JSON).expect("parse schema");
    let validator = Arc::new(jsonschema::validator_for(&schema_value).expect("compile schema"));
    let instance: JV = serde_json::from_str(INSTANCE_JSON).expect("parse instance");

    let lua = crate::lua::sandbox::make_engine().expect("engine");

    let mut g = c.benchmark_group("jsonschema");
    g.throughput(Throughput::Elements(1));

    g.bench_function("is_valid/library", |b| {
        b.iter(|| {
            let _ = validator.is_valid(black_box(&instance));
        });
    });

    let ident: mlua::Function = lua
        .create_function(|_, _v: mlua::Value| Ok(true))
        .expect("ident");
    let instance_lua_chunk: mlua::Function = lua
        .load(
            r#"return function() return { id = 1, name = "Alice", email = "alice@example.com", active = true } end"#,
        )
        .eval()
        .expect("seed chunk");
    let instance_lua: mlua::Value = instance_lua_chunk.call(()).expect("seed eval");

    g.bench_function("is_valid/into_out_lua", |b| {
        b.iter(|| {
            let _: bool = ident.call(black_box(instance_lua.clone())).unwrap();
        });
    });

    lua.globals()
        .set(
            "_BENCH_SCHEMA",
            lua.load(&format!(
                r#"return (require("vnd:jsonschema")).compile([==[{schema}]==])"#,
                schema = SCHEMA_JSON
            ))
            .eval::<mlua::Value>()
            .expect("compile via Lua"),
        )
        .expect("set _BENCH_SCHEMA");

    let through_chunk: mlua::Function = lua
        .load(r#"return function(v) return _BENCH_SCHEMA:is_valid(v) end"#)
        .eval()
        .expect("through chunk");

    g.bench_function("is_valid/through_lua", |b| {
        b.iter(|| {
            let _: bool = through_chunk.call(black_box(instance_lua.clone())).unwrap();
        });
    });

    g.finish();
}
