//! The declarations for app authors, written from the schema of what a script can reach
//! (`vos_lua::declarations::schema`: mlua-ir's IR of neoc-vm2's modules and of the `app` API, read from their Rust)
//! by mlua-ir-gen's Luau and TypeScript generators. What is written here is only the host's policy:
//!
//! - which modules an app is offered: all of them but [`vos_domain::NOT_FOR_APPS`] ([`offered`]);
//! - in Luau, an unusable type for each module and global an app is not given, the global `app`, and the ONE
//!   `declare require` (two do not merge, the last one wins) with an overload per offered module, the unusable type
//!   for each refused one, and `(path: string) -> any` for an app's own bundled modules;
//! - in TypeScript, the global `app`; what an app is not offered is not declared (a name that is not declared is
//!   the error).
//!
//! `types/generated/vos.d.luau` is for luau-lsp (the editor, `bun run lint:luau`, and the build of every app's
//! backend); `types/generated/vos.d.ts` is for a backend written in TypeScript and compiled to Luau by
//! `TypeScriptToLua`. Neither is loaded by the host. A module name has a colon, which is not portable in a file
//! name, so nothing here is a file per module.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use mlua_ir::ir::{Field, Function, Ir, Module, ModuleKind, Param, Returns, Ty, TypeBody, TypeDef};
use vos_domain::{GLOBALS_NOT_FOR_APPS, NOT_FOR_APPS};
use vos_lua::declarations::{APP_GLOBAL, APP_MODULE, LANGUAGE_TYPES};

/// What an app is offered of a schema.
#[derive(Debug, Clone, PartialEq)]
pub struct Offered {
    /// The schema with only the offered modules and the types they own.
    pub ir: Ir,
    /// The modules that were left out.
    pub refused: Vec<String>,
}

/// The type of a module's table: `lib:json` is `lib_json`.
fn module_type(name: &str) -> String {
    name.replace([':', '.', '-', '/'], "_")
}

/// The modules an app is offered: all of them but `not_for_apps`.
///
/// # Errors
/// The schema is not one the declarations can be written from whole: a module that is not offered to apps is not
/// there (the list names a module that is gone), there is no `app` API, a module is not reached with `require`, a
/// type belongs to no module, or an offered module uses a type of a module that is not offered. Each would
/// otherwise be a silently thinner (or dangling) definitions file.
pub fn offered(ir: &Ir, not_for_apps: &[&str]) -> Result<Offered, String> {
    for module in not_for_apps {
        if !ir.modules.iter().any(|m| m.name == *module) {
            return Err(format!("{module} is not offered to apps, but there is no such module"));
        }
    }
    if !ir.modules.iter().any(|m| m.name == APP_MODULE) {
        return Err(format!("the schema has no `{APP_MODULE}`: the app API is not linked"));
    }
    if let Some(module) = ir.modules.iter().find(|m| m.kind != ModuleKind::Require) {
        return Err(format!(
            "{} is not a `require` module: an app's globals are its environment's",
            module.name
        ));
    }
    let refused = |name: &str| not_for_apps.contains(&name);
    let modules: Vec<Module> = ir.modules.iter().filter(|m| !refused(&m.name)).cloned().collect();
    let mut types = Vec::new();
    for def in &ir.types {
        let Some(owner) = def.module.as_deref() else {
            return Err(format!("the type {} belongs to no module, so no section declares it", def.name));
        };
        if !ir.modules.iter().any(|m| m.name == owner) {
            return Err(format!("the type {} belongs to {owner}, which is not a module", def.name));
        }
        if !refused(owner) {
            types.push(def.clone());
        }
    }
    let offered = Ir { version: ir.version.clone(), modules, types };
    let declared: BTreeSet<&str> = offered.types.iter().map(|t| t.name.as_str()).collect();
    for name in references(&offered) {
        if !declared.contains(name.as_str()) && !LANGUAGE_TYPES.contains(&name.as_str()) {
            return Err(format!("an offered module uses the type {name}, which no offered module declares"));
        }
    }
    Ok(Offered { ir: offered, refused: not_for_apps.iter().map(|m| (*m).to_owned()).collect() })
}

/// Every named type the modules and types of `ir` mention.
fn references(ir: &Ir) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for module in &ir.modules {
        module.constants.iter().for_each(|c| in_ty(&c.ty, &mut found));
        module.objects.iter().for_each(|o| {
            found.insert(o.type_name.clone());
        });
        module.functions.iter().for_each(|f| in_function(f, &mut found));
    }
    for def in &ir.types {
        in_type(def, &mut found);
    }
    found
}

fn in_type(def: &TypeDef, found: &mut BTreeSet<String>) {
    match &def.body {
        TypeBody::Record { fields } | TypeBody::Options { fields } => {
            for field in fields {
                in_ty(&field.ty, found);
            }
        }
        TypeBody::StringEnum { .. } => {}
        TypeBody::TaggedUnion { variants, .. } => {
            variants.iter().flat_map(|v| &v.fields).for_each(|f| in_ty(&f.ty, found));
        }
        TypeBody::Error { variants, common, .. } => {
            variants.iter().flat_map(|v| &v.fields).chain(common).for_each(|f| in_ty(&f.ty, found));
        }
        TypeBody::Object { fields, methods, statics } => {
            for field in fields {
                in_ty(&field.ty, found);
            }
            methods.iter().chain(statics).for_each(|f| in_function(f, found));
        }
        TypeBody::Alias { ty } => in_ty(ty, found),
    }
}

fn in_function(function: &Function, found: &mut BTreeSet<String>) {
    function.params.iter().for_each(|p| in_ty(&p.ty, found));
    in_returns(&function.returns, found);
    if let Some(raised) = &function.raises {
        in_ty(raised, found);
    }
}

fn in_returns(returns: &Returns, found: &mut BTreeSet<String>) {
    match returns {
        Returns::None => {}
        Returns::One { ty } => in_ty(ty, found),
        Returns::Many { tys } => tys.iter().for_each(|ty| in_ty(ty, found)),
        Returns::Fallible { ok, err } => {
            in_ty(ok, found);
            in_ty(err, found);
        }
    }
}

fn in_ty(ty: &Ty, found: &mut BTreeSet<String>) {
    match ty {
        Ty::Nil
        | Ty::Boolean
        | Ty::Integer { .. }
        | Ty::Number
        | Ty::String
        | Ty::Unknown { .. }
        | Ty::Literal { .. } => {}
        Ty::Optional { inner } => in_ty(inner, found),
        Ty::List { item } => in_ty(item, found),
        Ty::Map { key, value } => {
            in_ty(key, found);
            in_ty(value, found);
        }
        Ty::Union { members } => members.iter().for_each(|m| in_ty(m, found)),
        Ty::Named { name } => {
            found.insert(name.clone());
        }
        Ty::Function { params, returns } => {
            for param in params {
                in_ty(&param.ty, found);
            }
            in_returns(returns, found);
        }
        Ty::Stream { item, error } => {
            in_ty(item, found);
            in_ty(error, found);
        }
    }
}

/// What a generator writes before the first module: its file with no module, less the line that says who
/// generated it (the header here says that).
fn prelude(file_of_nothing: &str) -> &str {
    file_of_nothing.split_once('\n').map_or("", |(_, rest)| rest).trim_start_matches('\n')
}

fn nothing(ir: &Ir) -> Ir {
    Ir { version: ir.version.clone(), modules: Vec::new(), types: Vec::new() }
}

// ---- Luau ---------------------------------------------------------------------------------------------------

const LUAU_HEADER: &str = "-- GENERATED by vos-codegen from the schema of the modules an app can reach (mlua-ir's, read from the Rust of\n\
-- neoc-vm2's std:, lib: and vnd: modules and of the `app` API in crates/vos-lua). Do not edit.\n\
-- Regenerate with `bun run codegen`; `bun run codegen:check` fails when this file is stale.\n\
--\n\
-- Luau declarations for luau-lsp (`bun run lint:luau`, the build of an app's backend, and editors: a definitions\n\
-- file, `--definitions:@vos=<this file>` of `luau-lsp`, `luau-lsp.types.definitionFiles` in an editor). Never loaded\n\
-- by the host. What it declares, in this order: the modules an app is offered (every module less the ones in\n\
-- NOT_FOR_APPS; `vos:app` is the `app` API), the types no value can be used through, the globals, and the ONE\n\
-- `declare require` (two do not merge) with an overload for every module an app is offered, and an unusable type\n\
-- for each module it is not.\n\n";

/// The prefix of the types the host's policy adds.
const PREFIX: &str = "vos_";

/// The type an unusable name has.
fn not_for_apps_type(name: &str) -> String {
    format!("{PREFIX}NotForApps_{}", name.replace(':', "_"))
}

/// The refused globals that are tables (a library): they get the table type below. Every other refused global is
/// a function (`print`, `loadstring`, ...) and gets the function type.
const REFUSED_LIBRARIES: &[&str] = &["buffer", "debug", "os", "vector"];

/// The modules and globals an app is not given, as types no value can be used through.
///
/// What is and is not a type error (measured with luau-lsp 1.70.1, see `scripts/fixtures/negative/wrong_requires.luau`):
/// - a refused module is `{ __not_for_apps: never }`: the first member access on what `require` returned is an error
///   that names it (`Key 'describe' not found in table 'vos_NotForApps_lib_test'`). The `require` call itself is NOT a
///   type error (a bare `require("lib:test")` that is never used checks clean); `bun run lint:luau` catches it by
///   scanning every literal `require`;
/// - a refused library global (`os`, `debug`, `buffer`, `vector`) has the same table type: a member access is an error;
/// - a refused function global (`print`, `loadstring`, ...) is `(__not_for_apps: never) -> never`: calling it with an
///   argument (`print("x")`, `pcall(print, "x")`, an alias of it) is an error ("Expected this to be unreachable"), and so
///   is calling it with none (the argument count). Naming it without calling it is not an error.
fn unusable(offered: &Offered) -> String {
    let mut out = String::from(
        "\n-- ===== not for apps =====\n\
-- A module in NOT_FOR_APPS (the Lua adapter refuses it when it runs) and a global the app's environment does not\n\
-- have. A module or a library is a table with one `never` field: the first member access is a type error naming it\n\
-- (requiring the module is not one: `bun run lint:luau` scans the `require`s). A function global takes a `never`\n\
-- argument and returns `never`: any call of it, direct or through `pcall`, is a type error.\n\n",
    );
    let table = "{ __not_for_apps: never }";
    for module in &offered.refused {
        let _ = writeln!(
            out,
            "--- `{module}` is not offered to apps.\nexport type {} = {table}\n",
            not_for_apps_type(module)
        );
    }
    for global in GLOBALS_NOT_FOR_APPS {
        let ty = if REFUSED_LIBRARIES.contains(global) { table } else { "(__not_for_apps: never) -> never" };
        let _ = writeln!(
            out,
            "--- `{global}` does not exist in an app's environment.\nexport type {} = {ty}\n",
            not_for_apps_type(global)
        );
    }
    out
}

fn luau_globals() -> String {
    let mut out = String::from("-- ===== globals =====\n\n");
    let _ = writeln!(
        out,
        "--- The `app` API, the same as `require(\"{APP_MODULE}\")`.\ndeclare {APP_GLOBAL}: {}\n",
        module_type(APP_MODULE)
    );
    for global in GLOBALS_NOT_FOR_APPS {
        let _ = writeln!(out, "declare {global}: {}", not_for_apps_type(global));
    }
    out.push('\n');
    out
}

/// The one `declare require`: the offered modules (`vos:app` among them), the refused modules (typed unusable),
/// and last `(path: string) -> any` for an app's own bundled modules (`require("./calc")`).
///
/// Why that last overload: luau-lsp resolves a relative `require` to the file's type only while `require` is the
/// built-in one, and a `declare require` replaces it (an intersection with `typeof(require)` brings the resolution
/// back for every call, and then fails every `vos:` and `std:` name as an unknown file). So in the editor, and in the
/// first pass of `bun run lint:luau`, a bundled module's type is `any`. The second pass of the lint cuts this whole
/// section out (the last one of the file), so the built-in `require` types the app's own modules there.
fn luau_require(offered: &Offered) -> String {
    let mut lines: Vec<String> = offered
        .ir
        .modules
        .iter()
        .map(|module| format!("((path: \"{}\") -> {})", module.name, module_type(&module.name)))
        .collect();
    lines.extend(
        offered
            .refused
            .iter()
            .map(|module| format!("((path: \"{module}\") -> {})", not_for_apps_type(module))),
    );
    lines.push("((path: string) -> any)".to_owned());
    format!("-- ===== require =====\n\ndeclare require: \n\t{}\n", lines.join("\n\t& "))
}

/// `types/generated/vos.d.luau`, from the schema.
///
/// # Errors
/// The schema is not one [`offered`] takes.
pub fn luau(ir: &Ir) -> Result<String, String> {
    Ok(luau_of(&offered(ir, NOT_FOR_APPS)?))
}

fn luau_of(offered: &Offered) -> String {
    let mut out = String::from(LUAU_HEADER);
    out.push_str(prelude(&mlua_ir_gen::luau::file(&nothing(&offered.ir))));
    for module in &offered.ir.modules {
        out.push('\n');
        out.push_str(&mlua_ir_gen::luau::module_section(&offered.ir, module));
    }
    out.push_str(&unusable(offered));
    out.push_str(&luau_globals());
    out.push_str(&luau_require(offered));
    out
}

/// `((path: "lib:json") -> lib_json)` -> `("lib:json", "lib_json")`.
fn overload(line: &str) -> Option<(String, String)> {
    let line = line.trim().trim_start_matches('&').trim();
    let rest = line.strip_prefix("((path: \"")?;
    let (module, rest) = rest.split_once("\") -> ")?;
    let ty = rest.strip_suffix(')')?;
    Some((module.to_owned(), ty.to_owned()))
}

/// The `(module, type)` of every `require` overload of a generated file, in order: what the Lua adapter's tests and
/// `bun run lint:luau` hold to the modules the host offers.
#[must_use]
pub fn require_overloads(definitions: &str) -> Vec<(String, String)> {
    let start = definitions.find("declare require:").unwrap_or(definitions.len());
    definitions[start..].lines().skip(1).filter_map(overload).collect()
}

// ---- TypeScript ---------------------------------------------------------------------------------------------

const TS_HEADER: &str = "// GENERATED by vos-codegen from the schema of the modules an app can reach (mlua-ir's, read from the Rust of\n\
// neoc-vm2's std:, lib: and vnd: modules and of the `app` API in crates/vos-lua). Do not edit.\n\
// Regenerate with `bun run codegen`; `bun run codegen:check` fails when this file is stale.\n\
//\n\
// TypeScript declarations for a backend written in TypeScript and compiled to Luau by TypeScriptToLua (the build of\n\
// `@vos/vite-plugin` copies this file into an app's `.app/server/`). Never loaded by the host. What it declares: every\n\
// type of the Luau declarations under the same name, a module for every `require` an app is offered\n\
// (`import * as fs from \"std:fs\"`), and the global `app`. A function that takes `this: void` is called with `.`, a\n\
// method of an interface with `:`. A module or a global an app is not offered is not declared.\n\n";

/// `types/generated/vos.d.ts`, from the schema.
///
/// # Errors
/// The schema is not one [`offered`] takes.
pub fn typescript(ir: &Ir) -> Result<String, String> {
    Ok(typescript_of(&offered(ir, NOT_FOR_APPS)?))
}

fn typescript_of(offered: &Offered) -> String {
    let ir = typescript_safe(&offered.ir);
    let mut out = String::from(TS_HEADER);
    out.push_str(prelude(&mlua_ir_gen::ts::file(&nothing(&ir))));
    for module in &ir.modules {
        out.push('\n');
        out.push_str(&mlua_ir_gen::ts::module_section(&ir, module));
    }
    let _ = write!(
        out,
        "\n// ===== globals =====\n\n/** The `app` API, the same as `require(\"{APP_MODULE}\")`. */\n\
declare const {APP_GLOBAL}: typeof import(\"{APP_MODULE}\")\n"
    );
    out
}

/// Words TypeScript does not take as the name of a parameter.
const TS_RESERVED: &[&str] = &[
    "arguments",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "eval",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

/// The schema as mlua-ir-gen's TypeScript generator can write it into a file `tsc` reads. Two things in a real
/// surface are not TypeScript as they stand, and the generator writes both through:
///
/// - a parameter named by a word of TypeScript (`diff.lines(old, new)`): it gets a trailing underscore, which
///   changes nothing for a caller (a parameter's name is not part of a call);
/// - a doc that holds `*/` (a glob in an example, `"src/**/*.rs"`), which would end the comment it is written in:
///   it is written `*\/`.
///
/// Both belong in the generator; this pass goes when it does them.
fn typescript_safe(ir: &Ir) -> Ir {
    let mut ir = ir.clone();
    for module in &mut ir.modules {
        safe_doc(&mut module.doc);
        module.constants.iter_mut().for_each(|c| {
            safe_doc(&mut c.doc);
            safe_ty(&mut c.ty);
        });
        module.functions.iter_mut().for_each(safe_function);
    }
    for def in &mut ir.types {
        safe_doc(&mut def.doc);
        match &mut def.body {
            TypeBody::Record { fields } | TypeBody::Options { fields } => {
                fields.iter_mut().for_each(safe_field);
            }
            TypeBody::StringEnum { variants } => variants.iter_mut().for_each(|v| safe_doc(&mut v.doc)),
            TypeBody::TaggedUnion { variants, .. } => variants.iter_mut().for_each(|v| {
                safe_doc(&mut v.doc);
                v.fields.iter_mut().for_each(safe_field);
            }),
            TypeBody::Error { variants, common, .. } => {
                for variant in variants {
                    safe_doc(&mut variant.doc);
                    variant.fields.iter_mut().for_each(safe_field);
                }
                common.iter_mut().for_each(safe_field);
            }
            TypeBody::Object { fields, methods, statics } => {
                fields.iter_mut().for_each(safe_field);
                methods.iter_mut().chain(statics).for_each(safe_function);
            }
            TypeBody::Alias { ty } => safe_ty(ty),
        }
    }
    ir
}

fn safe_doc(doc: &mut Option<String>) {
    if let Some(text) = doc {
        if text.contains("*/") {
            *text = text.replace("*/", "*\\/");
        }
    }
}

fn safe_field(field: &mut Field) {
    safe_doc(&mut field.doc);
    safe_ty(&mut field.ty);
}

fn safe_function(function: &mut Function) {
    safe_doc(&mut function.doc);
    safe_params(&mut function.params);
    safe_returns(&mut function.returns);
    if let Some(raised) = &mut function.raises {
        safe_ty(raised);
    }
}

fn safe_params(params: &mut [Param]) {
    for param in params {
        if TS_RESERVED.contains(&param.name.as_str()) {
            param.name.push('_');
        }
        safe_ty(&mut param.ty);
    }
}

fn safe_returns(returns: &mut Returns) {
    match returns {
        Returns::None => {}
        Returns::One { ty } => safe_ty(ty),
        Returns::Many { tys } => tys.iter_mut().for_each(safe_ty),
        Returns::Fallible { ok, err } => {
            safe_ty(ok);
            safe_ty(err);
        }
    }
}

fn safe_ty(ty: &mut Ty) {
    match ty {
        Ty::Nil
        | Ty::Boolean
        | Ty::Integer { .. }
        | Ty::Number
        | Ty::String
        | Ty::Unknown { .. }
        | Ty::Literal { .. }
        | Ty::Named { .. } => {}
        Ty::Optional { inner } => safe_ty(inner),
        Ty::List { item } => safe_ty(item),
        Ty::Map { key, value } => {
            safe_ty(key);
            safe_ty(value);
        }
        Ty::Union { members } => members.iter_mut().for_each(safe_ty),
        Ty::Function { params, returns } => {
            safe_params(params);
            safe_returns(returns);
        }
        Ty::Stream { item, error } => {
            safe_ty(item);
            safe_ty(error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua_ir::ir::{Constant, ObjectExport};

    fn module(name: &str) -> Module {
        Module {
            name: name.to_owned(),
            kind: ModuleKind::Require,
            doc: Some(format!("The module {name}.")),
            functions: Vec::new(),
            constants: Vec::new(),
            objects: Vec::new(),
        }
    }

    fn record(name: &str, owner: &str, fields: Vec<Field>) -> TypeDef {
        TypeDef {
            name: name.to_owned(),
            short_name: name.rsplit('_').next().unwrap().to_owned(),
            module: Some(owner.to_owned()),
            rust: "probe".to_owned(),
            doc: None,
            body: TypeBody::Record { fields },
        }
    }

    fn field(name: &str, ty: Ty) -> Field {
        Field { name: name.to_owned(), doc: None, ty, default: None, readonly: false }
    }

    /// `lib:probe` (offered) with a function and a type, `lib:test` (refused) with a type, and the app API.
    fn sample() -> Ir {
        let mut probe = module("lib:probe");
        probe.functions.push(Function {
            name: "open".to_owned(),
            doc: Some("Open one.".to_owned()),
            params: vec![Param { name: "path".to_owned(), ty: Ty::String, optional: false, variadic: false }],
            returns: Returns::One { ty: Ty::named("lib_probe_Thing") },
            raises: None,
            is_async: false,
        });
        let mut app = module(APP_MODULE);
        app.constants.push(Constant { name: "id".to_owned(), doc: None, ty: Ty::String });
        Ir {
            version: "0".to_owned(),
            modules: vec![probe, module("lib:test"), app],
            types: vec![
                record("lib_probe_Thing", "lib:probe", vec![field("size", Ty::Number)]),
                record("lib_test_Case", "lib:test", vec![]),
            ],
        }
    }

    #[test]
    fn a_refused_module_is_left_out_with_its_types() {
        let got = offered(&sample(), &["lib:test"]).unwrap();
        assert_eq!(
            got.ir.modules.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
            ["lib:probe", APP_MODULE]
        );
        assert_eq!(got.ir.types.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["lib_probe_Thing"]);
        assert_eq!(got.refused, ["lib:test"]);
    }

    #[test]
    fn a_schema_the_declarations_cannot_be_written_from_whole_is_an_error() {
        assert!(offered(&sample(), &["lib:gone"]).unwrap_err().contains("there is no such module"));

        let mut no_app = sample();
        no_app.modules.pop();
        assert!(offered(&no_app, &[]).unwrap_err().contains("the app API is not linked"));

        let mut globals = sample();
        globals.modules[0].kind = ModuleKind::Globals;
        assert!(offered(&globals, &[]).unwrap_err().contains("is not a `require` module"));

        let mut orphan = sample();
        orphan.types[0].module = None;
        assert!(offered(&orphan, &[]).unwrap_err().contains("belongs to no module"));

        // an offered module that hands out a type of a refused one would declare a name nothing defines
        let mut dangling = sample();
        dangling.types[0] = record(
            "lib_probe_Thing",
            "lib:probe",
            vec![field("case", Ty::optional(Ty::named("lib_test_Case")))],
        );
        assert!(offered(&dangling, &["lib:test"]).unwrap_err().contains("uses the type lib_test_Case"));
        let mut exported = sample();
        exported.modules[0]
            .objects
            .push(ObjectExport { name: "Case".to_owned(), type_name: "lib_test_Case".to_owned() });
        assert!(offered(&exported, &["lib:test"]).unwrap_err().contains("uses the type lib_test_Case"));
        assert!(offered(&dangling, &[]).is_ok());
    }

    #[test]
    fn the_luau_file_is_the_offered_modules_then_the_policy_and_one_require() {
        let luau = luau_of(&offered(&sample(), &["lib:test"]).unwrap());
        assert!(luau.starts_with("-- GENERATED by vos-codegen"), "{luau}");
        assert_eq!(luau.matches("GENERATED").count(), 1, "one header: {luau}");
        assert!(luau.contains("export type mlua_Stream<T, E> = {"), "the generator's own prelude: {luau}");
        assert!(luau.contains("-- ===== lib:probe =====\n\nexport type lib_probe_Thing = {"), "{luau}");
        assert!(luau.contains("\t--- Open one.\n\topen: (path: string) -> lib_probe_Thing,\n"), "{luau}");
        assert!(!luau.contains("-- ===== lib:test =====") && !luau.contains("lib_test_Case"), "{luau}");
        assert!(luau.contains("export type vos_NotForApps_lib_test = { __not_for_apps: never }\n"), "{luau}");
        assert!(luau.contains("\ndeclare app: vos_app\n"), "{luau}");
        assert_eq!(luau.lines().filter(|l| l.starts_with("declare require")).count(), 1);
        assert_eq!(
            require_overloads(&luau),
            [
                ("lib:probe".to_owned(), "lib_probe".to_owned()),
                ("vos:app".to_owned(), "vos_app".to_owned()),
                ("lib:test".to_owned(), "vos_NotForApps_lib_test".to_owned()),
            ]
        );
        assert!(luau.trim_end().ends_with("& ((path: string) -> any)"), "{luau}");
    }

    /// What mlua-ir-gen writes through and `tsc` refuses: a parameter named by a word of TypeScript, and a doc that
    /// ends its own comment.
    #[test]
    fn the_typescript_file_is_typescript_where_the_schema_is_not() {
        let mut ir = sample();
        let open = &mut ir.modules[0].functions[0];
        open.doc = Some("Open `src/**/*.rs`.".to_owned());
        open.params[0].name = "new".to_owned();
        let ts = typescript_of(&offered(&ir, &["lib:test"]).unwrap());
        assert!(ts.contains("  /** Open `src/**\\/*.rs`. */\n  export function open(this: void, new_: string): lib_probe_Thing\n"), "{ts}");
        // Luau has neither problem: the declarations say what the Rust says
        let luau = luau_of(&offered(&ir, &["lib:test"]).unwrap());
        assert!(
            luau.contains("\t--- Open `src/**/*.rs`.\n\topen: (new: string) -> lib_probe_Thing,\n"),
            "{luau}"
        );
    }

    #[test]
    fn the_typescript_file_declares_what_is_offered_and_nothing_else() {
        let ts = typescript_of(&offered(&sample(), &["lib:test"]).unwrap());
        assert!(ts.starts_with("// GENERATED by vos-codegen"), "{ts}");
        assert_eq!(ts.matches("GENERATED").count(), 1, "one header: {ts}");
        assert!(ts.contains("/// <reference types=\"@typescript-to-lua/language-extensions\" />"), "{ts}");
        assert!(ts.contains("interface lib_probe_Thing {\n  size: number\n}\n"), "{ts}");
        // a module's members are its exports, called without a `self`; a type of the module is there under its
        // short name too
        assert!(ts.contains("declare module \"lib:probe\" {\n  /** Open one. */\n  export function open(this: void, path: string): lib_probe_Thing\n  export type Thing = lib_probe_Thing\n}\n"), "{ts}");
        assert!(ts.contains("declare module \"vos:app\" {\n  export const id: string\n}\n"), "{ts}");
        assert!(ts.trim_end().ends_with("declare const app: typeof import(\"vos:app\")"), "{ts}");
        assert!(!ts.contains("lib:test") && !ts.contains("NotForApps") && !ts.contains("print"), "{ts}");
    }
}
