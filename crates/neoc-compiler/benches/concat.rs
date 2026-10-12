//! Loop-carried string append (`lang/DYNAMIC.md` §4.11): 4096 appends to one
//! local, lowered with the accumulator (`table.concat`) against the plain `..`
//! lowering. Both programs are compiled from neoc source and run on the
//! runtime's in-process `LuaJIT`; the baseline holds the string in a closure,
//! which is exactly what turns the rewrite off, and is otherwise identical. The
//! appended value is a local, not a literal, so each push pays the `.. ""`
//! operand check.
//!
//! Run with `cargo bench -p neoc-compiler --bench concat`.

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use mlua::{Function, Lua};
use neoc_compiler::{Target, transpile};
use neoc_vm::lua::sandbox;

const APPENDS: usize = 4096;

fn source(capture: &str) -> String {
    format!(
        "pub fn run() -> string {{
    let s = \"\";
    let chunk = \"0123456789abcdef\";
    {capture}
    for i in 0..{APPENDS} {{
        s += chunk;
    }}
    s
}}"
    )
}

/// Compile `src` and load it into a fresh sandboxed engine; return the engine
/// and its `run` function.
fn load(src: &str) -> (Lua, Function) {
    let out = transpile(src, Target::Lua);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    let lua = sandbox::make_engine().expect("engine");
    sandbox::load_source(&lua, &out.output)
        .set_name("=concat")
        .exec()
        .expect("load");
    let run = lua.globals().get("run").expect("run");
    (lua, run)
}

fn string_concat_4k(c: &mut Criterion) {
    let accumulated = load(&source(""));
    let plain = load(&source("let peek = || s;"));
    for (_, run) in [&accumulated, &plain] {
        let built = run.call::<mlua::LuaString>(()).expect("run");
        assert_eq!(built.as_bytes().len(), APPENDS * 16);
    }

    let mut group = c.benchmark_group("string_concat_4k");
    group.bench_function("table_concat", |b| {
        b.iter_batched(
            || (),
            |()| accumulated.1.call::<mlua::LuaString>(()),
            BatchSize::SmallInput,
        );
    });
    group.bench_function("plain_dotdot", |b| {
        b.iter_batched(
            || (),
            |()| plain.1.call::<mlua::LuaString>(()),
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group!(benches, string_concat_4k);
criterion_main!(benches);
