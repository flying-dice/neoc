//! The bundle-only `require`.
//!
//! There is no filesystem. `require("./calc")` and `require("../shared/util")` resolve (relative to the file
//! that asks) to `calc.luau` / `calc/init.luau` among the files of the app's own verified package;
//! `require("@agentos/app")` is the capability table; `require("@agentos/<name>")` is a host module the host
//! registered (the generated `ui` constructors). Anything else is an error. Modules run once and are cached;
//! a cycle is an error naming the files.

use crate::vm::Shared;
use mlua::{Lua, Value as LuaValue};
use std::sync::Arc;

/// Resolve `spec` against the file `from` (a package path like `lib/util.luau`): the module path without
/// extension. Pure, so the rules are tested directly.
pub(crate) fn resolve(from: &str, spec: &str) -> Result<String, String> {
    if !(spec.starts_with("./") || spec.starts_with("../")) {
        return Err(format!(
            "require(\"{spec}\"): paths must be relative (\"./x\", \"../x\") or a host module (\"@agentos/app\")"
        ));
    }
    let mut parts: Vec<&str> = from.rsplit_once('/').map_or(vec![], |(dir, _)| dir.split('/').collect());
    for segment in spec.trim_end_matches(".luau").split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts
                    .pop()
                    .ok_or_else(|| format!("require(\"{spec}\") from `{from}` leaves the app package"))?;
            }
            s => parts.push(s),
        }
    }
    if parts.is_empty() {
        return Err(format!("require(\"{spec}\") names no module"));
    }
    Ok(parts.join("/"))
}

pub(crate) fn install(lua: &Lua, sh: &Arc<Shared>) -> mlua::Result<()> {
    let sh = sh.clone();
    let require = lua.create_async_function(move |lua, spec: String| {
        // read the caller NOW, while its frame is still on the stack
        let caller = caller_chunk(&lua);
        let sh = sh.clone();
        async move { load(&lua, &sh, &caller, &spec).await }
    })?;
    lua.globals().set("require", require)
}

/// The chunk that called `require`: the nearest frame above us that is app (or host module) code. Frames of
/// mlua's own async helper sit in between, so look a few levels up.
fn caller_chunk(lua: &Lua) -> String {
    (1..12)
        .find_map(|level| {
            lua.inspect_stack(level, |d| d.source().source.map(std::borrow::Cow::into_owned))
                .flatten()
                .filter(|s| s.starts_with('=') && !s.starts_with("=[") && !s.contains("__mlua"))
        })
        .unwrap_or_default()
}

/// The package path of a chunk named `=<app>/<path>`; `None` for host modules and unknown chunks.
fn package_path<'a>(sh: &Shared, chunk: &'a str) -> Option<&'a str> {
    chunk.strip_prefix('=')?.strip_prefix(&format!("{}/", sh.app))
}

async fn load(lua: &Lua, sh: &Arc<Shared>, caller: &str, spec: &str) -> mlua::Result<LuaValue> {
    if let Some(name) = spec.strip_prefix("@agentos/") {
        if name == "app" {
            return sh
                .app_table
                .lock()
                .clone()
                .map(LuaValue::Table)
                .ok_or_else(|| mlua::Error::runtime("the app table is gone (the app was unloaded)"));
        }
        let source = sh
            .host_modules
            .get(name)
            .ok_or_else(|| mlua::Error::runtime(format!("unknown host module `@agentos/{name}`")))?;
        return run_module(lua, sh, &format!("@agentos/{name}"), format!("=@agentos/{name}"), source).await;
    }

    let from = package_path(sh, caller).ok_or_else(|| {
        mlua::Error::runtime(format!(
            "require(\"{spec}\"): relative requires work only inside the app's own files"
        ))
    })?;
    let base = resolve(from, spec).map_err(mlua::Error::runtime)?;
    let candidates = [format!("{base}.luau"), format!("{base}/init.luau")];
    let Some((path, source)) =
        candidates.iter().find_map(|c| sh.package.files.get(c).map(|s| (c.clone(), s)))
    else {
        return Err(mlua::Error::runtime(format!(
            "require(\"{spec}\") from `{from}`: no module `{}` in the app package",
            candidates[0]
        )));
    };
    run_module(lua, sh, &path, sh.chunk_name(&path), source).await
}

async fn run_module(
    lua: &Lua,
    sh: &Arc<Shared>,
    key: &str,
    chunk: String,
    source: &[u8],
) -> mlua::Result<LuaValue> {
    if let Some(done) = sh.modules.lock().get(key) {
        return Ok(done.clone());
    }
    if !sh.loading.lock().insert(key.to_owned()) {
        let chain: Vec<String> = sh.loading.lock().iter().cloned().collect();
        return Err(mlua::Error::runtime(format!(
            "cyclic require of `{key}` (loading: {})",
            chain.join(", ")
        )));
    }
    let result = lua.load(source).set_name(chunk).call_async::<LuaValue>(()).await;
    sh.loading.lock().remove(key);
    let value = match result? {
        LuaValue::Nil => LuaValue::Boolean(true),
        v => v,
    };
    sh.modules.lock().insert(key.to_owned(), value.clone());
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::resolve;

    #[test]
    fn relative_paths_resolve_against_the_requiring_file() {
        assert_eq!(resolve("app.luau", "./calc").unwrap(), "calc");
        assert_eq!(resolve("lib/util.luau", "./helpers/x").unwrap(), "lib/helpers/x");
        assert_eq!(resolve("lib/util.luau", "../calc").unwrap(), "calc");
        assert_eq!(resolve("a/b/c.luau", "../../d/e").unwrap(), "d/e");
        assert_eq!(resolve("app.luau", "./calc.luau").unwrap(), "calc", "an explicit extension is fine");
    }

    #[test]
    fn nothing_escapes_the_package_and_only_relative_or_host_paths_are_allowed() {
        assert!(resolve("app.luau", "../etc/passwd").unwrap_err().contains("leaves the app package"));
        assert!(resolve("lib/x.luau", "../../y").is_err());
        for bad in ["calc", "/etc/passwd", "C:\\x", "", "@other/app"] {
            assert!(resolve("app.luau", bad).unwrap_err().contains("must be relative"), "{bad}");
        }
        assert!(resolve("app.luau", "./").is_err());
    }
}
