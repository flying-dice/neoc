//! `types/generated/vos.d.ts`: the TypeScript declarations of what an app's backend is offered, for a backend written
//! in TypeScript and compiled to Luau by `TypeScriptToLua` (the build of `@vos/vite-plugin`, and editors).
//!
//! It is written from the same tree as `types/generated/vos.d.luau`: neoc's modules as neoc-vm recorded them
//! (`types/neoc.ast.json`, read as `neoc_decl`'s types), and what vos adds to them, which is written by hand in Luau
//! (the `app` API) and read into that tree. The same modules, types and documentation, so the two cannot differ, and
//! `bun run codegen:check` fails when either is stale. [`emit`] writes the tree as TypeScript.

mod emit;

pub use emit::declarations;

#[cfg(test)]
mod tests {
    use super::{declarations, emit::translate};
    use crate::{luau_definitions, neoc_modules, require_overloads, NEOC_AST};

    /// The TypeScript of a definitions file written in Luau.
    fn from_luau(luau: &str) -> Result<String, String> {
        translate(&neoc_decl::parse::sections(luau)?)
    }

    /// A definitions file of one module, `lib:probe`, with `body` as the rest of its section.
    fn file(body: &str) -> String {
        format!(
            "-- header\n\n-- ===== lib:probe =====\n\n{body}\n-- ===== not for apps =====\n\n\
--- `lib:test` is not offered to apps.\nexport type vos_NotForApps_lib_test = {{ __not_for_apps: never }}\n\n\
export type vos_NotForApps_print = (__not_for_apps: never) -> never\n\n\
-- ===== globals =====\n\n--- The probe.\ndeclare probe: lib_probe\n\ndeclare print: vos_NotForApps_print\n\n\
-- ===== require =====\n\ndeclare require: \n\t((path: \"lib:probe\") -> lib_probe)\n\
\t& ((path: \"lib:test\") -> vos_NotForApps_lib_test)\n\t& ((path: string) -> any)\n"
        )
    }

    fn translated(body: &str) -> String {
        from_luau(&file(body)).unwrap()
    }

    #[test]
    fn a_method_is_called_on_its_value_and_a_function_is_not() {
        let ts = translated(
            "export type lib_probe_Thing = {\n\t--- Its size.\n\tsize: (self: lib_probe_Thing) -> number,\n\
\tmake: (name: string) -> lib_probe_Thing,\n}\n\n\
export type lib_probe = {\n\t--- Open one.\n\topen: (path: string, mode: string?) -> (lib_probe_Thing?, string?),\n\
\tclose: (self: lib_probe) -> (),\n}\n",
        );
        // a method: `self` is the interface's own `this`; a function says it has none
        assert!(ts.contains("interface lib_probe_Thing {\n  /** Its size. */\n  size(): number\n"), "{ts}");
        assert!(ts.contains("  make(this: void, name: string): lib_probe_Thing\n"), "{ts}");
        // a module's members are its exports; a result of several values is a LuaMultiReturn
        assert!(ts.contains("declare module \"lib:probe\" {\n  /** Open one. */\n  export function open(this: void, path: string, mode?: string): LuaMultiReturn<[lib_probe_Thing | undefined, string | undefined]>\n"), "{ts}");
        assert!(ts.contains("  export function close(this: lib_probe): void\n"), "{ts}");
        // a type of the module is also there under its short name
        assert!(ts.contains("  export type Thing = lib_probe_Thing\n"), "{ts}");
    }

    #[test]
    fn every_shape_of_type_has_its_typescript() {
        let ts = translated(
            "export type lib_probe_Kind = \"a\" | \"b\"\n\n\
export type lib_probe_Row = { id: number, tag: string?, [\"until\"]: number?, gone: nil }\n\n\
export type lib_probe = {\n\
\trows: { lib_probe_Row },\n\
\tnames: { [string]: lib_probe_Kind }?,\n\
\tbytes: buffer,\n\
\tjoin: (first: string, ...string) -> string,\n\
\tspawn: (fn: any, ...any) -> ...any,\n\
\tfirst: (path: string | number, ...(string | number)) -> (any?, ...any),\n\
\ttx: <T>(body: () -> T) -> T,\n\
\tserve: (handler: (req: lib_probe_Row) -> ...(string | { status: number? } | nil)) -> (() -> ())?,\n\
\tnotify: ((note: lib_probe_Row) -> ()) & ((self: lib_probe, note: lib_probe_Row) -> ()),\n\
\tskip: (from: number?, to: number) -> (),\n\
\tnew: (default: boolean, ...any) -> lib_probe_Row,\n\
}\n",
        );
        for line in [
            "type lib_probe_Kind = \"a\" | \"b\"\n",
            // a small table type is an interface when it is declared, with what may be absent optional
            "interface lib_probe_Row {\n  id: number\n  tag?: string\n  until?: number\n  gone?: undefined\n}\n",
            "  export const rows: lib_probe_Row[]\n",
            "  export const names: { [key: string]: lib_probe_Kind } | undefined\n",
            "  export const bytes: LuaBuffer\n",
            "  export function join(this: void, first: string, ...args: string[]): string\n",
            "  export function spawn(this: void, fn: any, ...args: any[]): LuaMultiReturn<any[]>\n",
            "  export function first(this: void, path: string | number, ...args: (string | number)[]): LuaMultiReturn<[any, ...any[]]>\n",
            "  export function tx<T>(this: void, body: (this: void) => T): T\n",
            "  export function serve(this: void, handler: (this: void, req: lib_probe_Row) => LuaMultiReturn<(string | { status?: number } | undefined)[]>): ((this: void) => void) | undefined\n",
            // overloads, each with its own way of being called
            "  export function notify(this: void, note: lib_probe_Row): void\n  export function notify(this: lib_probe, note: lib_probe_Row): void\n",
            // optional only when nothing required follows
            "  export function skip(this: void, from: number | undefined, to: number): void\n",
            // a word of TypeScript: a parameter is renamed, a member is declared under another name and exported under its own
            "  function new_(this: void, default_: boolean, ...args: any[]): lib_probe_Row\n",
            "  export { new_ as new }\n",
        ] {
            assert!(ts.contains(line), "missing {line:?} in:\n{ts}");
        }
    }

    #[test]
    fn what_an_app_is_not_offered_is_not_declared() {
        let ts = translated("export type lib_probe = {\n\tid: string,\n}\n");
        assert!(ts.contains("/** The probe. */\ndeclare const probe: lib_probe\n"), "{ts}");
        assert!(!ts.contains("lib:test") && !ts.contains("NotForApps") && !ts.contains("print"), "{ts}");
    }

    #[test]
    fn what_is_not_translated_is_an_error_that_says_where() {
        let failed = |body: &str| from_luau(&file(body)).unwrap_err();
        let module = "export type lib_probe = {\n\tid: string,\n}\n";
        assert!(failed("export type lib_probe = {\n\tco: thread,\n}\n")
            .contains("the Luau type `thread` has no TypeScript"));
        assert!(failed("export type lib_probe = {\n\tco: typeof(print),\n}\n")
            .contains("line 6: `typeof` is not translated"));
        assert!(failed("export type lib_probe = {\n\tco: lib_other.Thing,\n}\n")
            .contains("line 6: `lib_other` is followed by"));
        assert!(
            failed("export type lib_probe = {\n\ttext: \"a\\\"b\",\n}\n").contains("an escape in the string")
        );
        assert!(failed("export type lib_probe = string\n").contains("is not a table of members"));
        assert!(failed(&format!("{module}local x = y\n")).contains("expected `export type` or `declare`"));
        assert!(failed("export type lib_other = {}\n")
            .contains("`lib_probe`, the type of `lib:probe`, is not declared"));
        assert!(from_luau("-- nothing\n").unwrap_err().contains("no `not for apps` section"));
    }

    /// The committed declarations: every module an app is offered is a TypeScript module, and nothing else is.
    #[test]
    fn the_declarations_of_the_repository_translate_whole() {
        let json = std::fs::read_to_string(crate::repo_root().join(NEOC_AST)).unwrap();
        let modules = neoc_modules(&json).unwrap();
        let luau = luau_definitions(&modules).unwrap();
        let ts = declarations(&modules).unwrap();
        let modules: Vec<&str> =
            ts.lines().filter_map(|l| l.strip_prefix("declare module \"")?.strip_suffix("\" {")).collect();
        let offered: Vec<String> = require_overloads(&luau)
            .into_iter()
            .filter(|(_, ty)| !ty.starts_with("vos_NotForApps_"))
            .map(|(module, _)| module)
            .collect();
        assert_eq!(modules, offered);
        assert!(
            modules.contains(&"vos:app") && modules.contains(&"std:fs") && !modules.contains(&"lib:test")
        );
        assert!(ts.contains("declare const app: vos_AppApi\n"));
        for global in vos_domain::GLOBALS_NOT_FOR_APPS {
            assert!(!ts.contains(&format!("declare const {global}:")), "{global} is declared");
        }
    }
}
