#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test harness
//! `#t` of a table is whatever border Lua finds. On LuaJIT a script could forge a table whose `#` was 2^29 while it
//! held four rows, and a buffer presized from it asked the allocator for gigabytes: a failed allocation aborts the
//! host (on a machine that cannot back the request; a laptop overcommits, so a call passes there even when the code
//! is wrong). Luau's `#` does not probe the hash part for a chain of integer keys, so the same forgery reports a small
//! border there: [`a_table_that_claims_a_huge_length_does_not_size_a_buffer`] checks that, as a canary (red if a
//! forged `#` is possible again), and still runs the functions over the forged table. A test allocator would need
//! `unsafe` to see the request, which the crate does not use; instead [`no_buffer_is_presized_from_a_table_length`]
//! reads the source of the crate for the pattern.

use neoc_vm::lua::sandbox::make_engine;

/// A table of four values whose `#` is about 2^29. `#t` finds a border by probing doubling keys when the table has
/// a hash part (here: 40 keys inserted and removed, so the nodes stay allocated), so a chain of keys 5, 10, 20, ...
/// makes the probe run to 2^29.
const FORGE: &str = r#"
    local function huge(v)
      local t = { v, v, v, v }
      for i = 1, 40 do t["k" .. i] = 0 end
      for i = 1, 40 do t["k" .. i] = nil end
      local m = 5
      while m < 2 ^ 29 do t[m] = v m = m * 2 end
      return t
    end
"#;

#[test]
fn a_table_that_claims_a_huge_length_does_not_size_a_buffer() {
    let lua = make_engine().unwrap();
    let forged: f64 = lua
        .load(format!("{FORGE} return #huge('x')"))
        .eval()
        .unwrap();
    // on LuaJIT this was 335544320 (the premise of the test); Luau reports the border it finds in the array part
    assert!(
        forged < 1e6,
        "the forged table reports {forged}: a forged `#` is possible again, so the calls below size buffers from it"
    );
    // every function of lib:diff and lib:args that reads a table of rows or arguments
    lua.load(format!(
        r#"
        {FORGE}
        local diff = require("lib:diff")
        for name, f in pairs(diff) do
          if type(f) == "function" then
            pcall(f, huge({{ op = "equal", value = "x" }}))
            pcall(f, huge({{ op = "equal", value = "x" }}), huge({{ op = "equal", value = "x" }}))
            pcall(f, "a\nb", huge({{ op = "equal", value = "x" }}))
          end
        end
        pcall(require("lib:args").parse, huge("x"), {{ name = "x" }})
        "#
    ))
    .exec()
    .unwrap();
}

/// No `with_capacity(..)` or `reserve(..)` takes a table's length (`raw_len()`), directly or through a variable
/// bound from it: a forged `#t` makes it an allocation of gigabytes. Loop to the length (a missing row ends the
/// loop) and let the `Vec` grow, or cap the presize.
#[test]
fn no_buffer_is_presized_from_a_table_length() {
    let mut sources = Vec::new();
    collect(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut sources,
    );
    assert!(sources.len() > 50, "found only {} sources", sources.len());
    // `let len = t.raw_len();` and, within the next few lines, a buffer sized by `len`
    let bound =
        regex::Regex::new(r"let\s+(?:mut\s+)?(\w+)\s*(?::[^=]+)?=[^;]*\braw_len\(\)").unwrap();
    let sizing = regex::Regex::new(r"(?:with_capacity|reserve|reserve_exact)\(([^;]*)\)").unwrap();
    let line_of = |text: &str, at: usize| text[..at].matches('\n').count();
    let mut found = Vec::new();
    for (path, text) in &sources {
        let names: Vec<(&str, usize)> = bound
            .captures_iter(text)
            .map(|c| {
                (
                    c.get(1).unwrap().as_str(),
                    line_of(text, c.get(0).unwrap().start()),
                )
            })
            .collect();
        for call in sizing.captures_iter(text) {
            let argument = call.get(1).unwrap().as_str();
            let line = line_of(text, call.get(0).unwrap().start());
            let uses_name = names.iter().any(|&(name, defined)| {
                (defined..=defined + 12).contains(&line)
                    && regex::Regex::new(&format!(r"(?:^|[^.\w]){}\b", regex::escape(name)))
                        .unwrap()
                        .is_match(argument)
            });
            if argument.contains("raw_len") || uses_name {
                found.push(format!(
                    "{}:{}: {}",
                    path.display(),
                    line + 1,
                    call.get(0).unwrap().as_str()
                ));
            }
        }
    }
    assert!(
        found.is_empty(),
        "presized from a Lua table's length:\n{}",
        found.join("\n")
    );
}

fn collect(dir: &std::path::Path, out: &mut Vec<(std::path::PathBuf, String)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path).unwrap();
            out.push((path, text));
        }
    }
}
