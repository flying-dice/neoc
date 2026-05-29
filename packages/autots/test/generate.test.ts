import { test, expect } from "bun:test";
import * as path from "node:path";
import { generate, generateModule, generateGlobals, generateProject, moduleSlug } from "../src/generate.js";

const fixture = path.join(import.meta.dir, "fixtures", "sample.ts");
const ambient = path.join(import.meta.dir, "fixtures", "ambient.d.ts");

/** Exactly one top-level `extern {` opener. */
function externCount(out: string): number {
  return out.split("\n").filter((l) => /^extern \{$/.test(l)).length;
}

test("entry source file becomes one extern mod with structs + free fns", () => {
  const out = generate({ sourceFilePaths: [fixture] });

  // no wrapper/shim leakage from the obsolete model
  expect(out).not.toContain("inner: Unknown");
  expect(out).not.toContain("js_get");
  expect(out).not.toContain("js_call");
  expect(out).not.toContain("class ");
  expect(out).not.toContain("set_");

  // exactly one top-level extern block
  expect(externCount(out)).toBe(1);
  expect(out).toContain("mod sample {");

  // interface → struct with fields (no impl, no methods)
  expect(out).toContain("struct User { id: number, name: string }");

  // free functions → bodyless fns
  expect(out).toContain("fn greet(name: string) -> string;");
  expect(out).toContain("fn log(message: string);"); // void -> no arrow
  // `User | undefined` strips null then resolves to the emitted struct (policy:
  // non-null base, then known struct).
  expect(out).toContain("fn findUser(id: number) -> User;");
  expect(out).toContain("fn tags() -> [string];"); // string[] -> list

  // const exports skipped
  expect(out).not.toContain("VERSION");
});

test("--host is accepted but ignored (no abi string)", () => {
  const out = generate({ sourceFilePaths: [fixture] }, { host: "node" });
  expect(out).not.toContain('extern "');
  expect(out).toContain("extern {");
});

test("ambient modules become #[extern(path = …)] mods", () => {
  const out = generate({ sourceFilePaths: [ambient] });

  expect(externCount(out)).toBe(1);
  expect(out).toContain('#[extern(path = "fake-lib")]');
  expect(out).toContain("mod fake_lib {");
  expect(out).toContain("struct Opts { verbose: bool }");
  expect(out).toContain("fn doThing(name: string) -> number;");
  expect(out).toContain("fn run(opts: Opts);"); // known struct param keeps its name
  expect(out).toContain('#[extern(path = "fake-lib:sub")]');
  expect(out).toContain("mod sub {");
  expect(out).toContain("fn helper() -> bool;");
});

const edge = path.join(import.meta.dir, "fixtures", "edge.d.ts");

test("hardening: methods become bodyless impl fns; non-identifier members skipped", () => {
  const out = generate({ sourceFilePaths: [edge] });

  expect(out).toContain("struct Widget {");
  expect(out).toContain("id: number");
  expect(out).toContain("impl Widget {");
  // void methods, no arrow
  expect(out).toContain("fn reset(self);");
  expect(out).toContain("fn resize(self, w: number, h: number);");
  // returns a known struct
  expect(out).toContain("fn clone(self) -> Widget;");
  // unit/void param becomes Unknown, never `()`
  expect(out).toContain("fn nudge(self, by: Unknown);");
  expect(out).not.toContain(": ()");
  // non-identifier member/export skipped
  expect(out).not.toContain("fn $(");
  // const export skipped
  expect(out).not.toContain("VERSION");
});

const globalsFx = path.join(import.meta.dir, "fixtures", "globals.d.ts");

test("declare global surface becomes #[extern(ambient)] items", () => {
  const out = generate({ sourceFilePaths: [globalsFx] });

  // exactly one top-level extern block (ambient items live alongside mods)
  expect(externCount(out)).toBe(1);

  // global function → ambient fn; `GResponse` return resolves to the emitted struct
  expect(out).toContain("#[extern(ambient)]\n    pub fn gfetch(url: string) -> GResponse;");

  // global interfaces → ambient structs
  expect(out).toContain("#[extern(ambient)]\n    pub struct GResponse { status: number }");
  expect(out).toContain("#[extern(ambient)]\n    pub struct GConsole;");
  // an interface with methods gets an impl (no per-impl attribute; ambient propagates)
  expect(out).toContain("impl GConsole {");
  expect(out).toContain("fn log(self, s: string);");

  // namespace-object var → #[extern(ambient, path = "<VarName>")] mod with its methods, self-less
  expect(out).toContain('#[extern(ambient, path = "gconsole")]');
  expect(out).toContain("mod gconsole {");
  expect(out).toContain("fn log(s: string);");
});

test("--no-globals drops the declare global surface", () => {
  const out = generate({ sourceFilePaths: [globalsFx] }, { globals: false });
  expect(out).not.toContain("gfetch");
  expect(out).not.toContain("GResponse");
  expect(out).not.toContain("GConsole");
  expect(out).not.toContain('path = "gconsole"');
  // the fixture has no other surface, so the block is empty
  expect(out).toContain("extern {\n}");
});

const topGlobalsFx = path.join(import.meta.dir, "fixtures", "topglobals.d.ts");

test("top-level ambient globals of a script file become #[extern(ambient)] items", () => {
  // A global script (no top-level import/export) declares its globals directly,
  // not inside `declare global { … }` — this is where `console`/`fetch` live.
  const out = generate({ sourceFilePaths: [topGlobalsFx] });

  expect(externCount(out)).toBe(1);

  // top-level `declare function` → ambient fn
  expect(out).toContain("#[extern(ambient)]\n    pub fn tfetch(url: string) -> TResponse;");
  // top-level `interface` → ambient struct (+ impl for the method-carrying one)
  expect(out).toContain("#[extern(ambient)]\n    pub struct TResponse { status: number }");
  expect(out).toContain("#[extern(ambient)]\n    pub struct TConsole;");
  expect(out).toContain("impl TConsole {");
  // top-level `declare var` of an object type → ambient mod rooted at its name
  expect(out).toContain('#[extern(ambient, path = "tconsole")]');
  expect(out).toContain("mod tconsole {");
  expect(out).toContain("fn log(s: string);");
});

test("--no-globals drops top-level script globals too", () => {
  const out = generate({ sourceFilePaths: [topGlobalsFx] }, { globals: false });
  expect(out).not.toContain("tfetch");
  expect(out).not.toContain("TResponse");
  expect(out).not.toContain('path = "tconsole"');
  expect(out).toContain("extern {\n}");
});

const genericsFx = path.join(import.meta.dir, "fixtures", "generics.d.ts");

test("generic surfaces emit type parameters, instantiations, and resolve in-scope refs", () => {
  const out = generate({ sourceFilePaths: [genericsFx] });

  // interface type params on the struct header, and resolved in fields/methods
  expect(out).toContain("pub struct Box<T> { value: T }");
  expect(out).toContain("impl Box<T> {");
  expect(out).toContain("pub fn get(self) -> T;");
  // a method's own type param is layered over the struct's and printed in its header
  expect(out).toContain("pub fn map<U>(self, f: fn(T) -> U) -> Box<U>;");

  // class type params on struct + impl; constructor and methods resolve them
  expect(out).toContain("pub struct Container<K, V> { size: number }");
  expect(out).toContain("impl Container<K, V> {");
  expect(out).toMatch(/#\[extern\(constructor\)\]\n\s*pub fn new\(key: K\);/);
  // `V | undefined` folds to `V`
  expect(out).toContain("pub fn get(self, key: K) -> V;");
  expect(out).toContain("pub fn set(self, key: K, value: V) -> Container<K, V>;");

  // free function with its own type param + a generic return
  expect(out).toContain("pub fn wrap<T>(value: T) -> Box<T>;");
});

const classesFx = path.join(import.meta.dir, "fixtures", "classes.d.ts");

test("classes become struct + impl with a #[extern(constructor)] new", () => {
  const out = generate({ sourceFilePaths: [classesFx] });

  expect(externCount(out)).toBe(1);
  expect(out).toContain('#[extern(path = "demo-db")]');

  expect(out).toContain("struct Database;");
  expect(out).toContain("impl Database {");
  // trailing optional ctor params dropped; all optional → keep first; no return type.
  // the attribute breaks onto its own line above the fn (canonical neoc fmt).
  expect(out).toMatch(/#\[extern\(constructor\)\]\n\s*pub fn new\(filename: string\);/);
  expect(out).toContain("fn run(self, sql: string) -> Changes;");
  expect(out).toContain("fn count(self) -> number;");

  // method-less interface → struct with fields, no impl
  expect(out).toContain("struct Changes { changes: number }");

  // no-constructor class → 0-arg new
  expect(out).toContain("struct Ping;");
  expect(out).toMatch(/#\[extern\(constructor\)\]\n\s*pub fn new\(\);/);

  // no bodies, no shim
  expect(out).not.toContain("js_new");
  expect(out).not.toContain("inner:");
  expect(out).not.toContain("class ");
  // `export default` must not leak a `struct default`
  expect(out).not.toContain("struct default");
});

test("hardening: a real package generates without throwing", () => {
  const pkg = path.join(import.meta.dir, "..", "node_modules", "@types", "yargs", "index.d.ts");
  expect(() => generate({ sourceFilePaths: [pkg] })).not.toThrow();
});

// --- project-aware API: per-module + globals split ---

test("moduleSlug sanitizes a specifier to a neoc identifier", () => {
  expect(moduleSlug("bun:sqlite")).toBe("bun_sqlite");
  expect(moduleSlug("node:fs")).toBe("node_fs");
  expect(moduleSlug("@scope/pkg")).toBe("_scope_pkg");
  expect(moduleSlug("some-lib.sub")).toBe("some_lib_sub");
});

test("generateModule emits one ambient module's extern file", () => {
  const out = generateModule({ sourceFilePaths: [ambient] }, "fake-lib");
  expect(out).not.toBeNull();
  const file = out!;

  // its own file header + a single extern block carrying the path attr
  expect(file.startsWith("// @generated by autots — do not edit.")).toBe(true);
  expect(externCount(file)).toBe(1);
  expect(file).toContain('#[extern(path = "fake-lib")]');
  expect(file).toContain("mod fake_lib {");
  expect(file).toContain("struct Opts { verbose: bool }");
  expect(file).toContain("fn doThing(name: string) -> number;");

  // a different module in the same sources is NOT pulled into this file
  expect(file).not.toContain('path = "fake-lib:sub"');
  expect(file).not.toContain("fn helper");
});

test("generateModule narrows to exactly the requested specifier", () => {
  const sub = generateModule({ sourceFilePaths: [ambient] }, "fake-lib:sub");
  expect(sub).not.toBeNull();
  expect(sub!).toContain('#[extern(path = "fake-lib:sub")]');
  expect(sub!).toContain("fn helper() -> bool;");
  expect(sub!).not.toContain("doThing"); // the other module stays out
});

test("generateModule returns null for an undeclared specifier", () => {
  expect(generateModule({ sourceFilePaths: [ambient] }, "no-such-module")).toBeNull();
});

test("generateGlobals emits the declare-global surface as its own file", () => {
  const out = generateGlobals({ sourceFilePaths: [globalsFx] });

  expect(out.startsWith("// @generated by autots — do not edit.")).toBe(true);
  expect(externCount(out)).toBe(1);

  // ambient items only — no `declare module` surface leaks in
  expect(out).toContain("#[extern(ambient)]\n    pub fn gfetch(url: string) -> GResponse;");
  expect(out).toContain("#[extern(ambient)]\n    pub struct GResponse { status: number }");
  expect(out).toContain('#[extern(ambient, path = "gconsole")]');
  expect(out).toContain("mod gconsole {");
});

test("generateGlobals on globals-free sources emits an empty extern block", () => {
  const out = generateGlobals({ sourceFilePaths: [ambient] });
  expect(out).toContain("extern {\n}");
});

test("generateProject emits one file: each module's surface, then the globals", () => {
  const { neoc, modules } = generateProject(
    { sourceFilePaths: [ambient, topGlobalsFx] },
    ["fake-lib", "fake-lib:sub"],
  );

  // a single header + a single extern block holds everything
  expect(neoc.startsWith("// @generated by autots — do not edit.")).toBe(true);
  expect(externCount(neoc)).toBe(1);

  // every requested module resolved, in config order
  expect(modules).toEqual(["fake-lib", "fake-lib:sub"]);

  // both module surfaces are present
  expect(neoc).toContain('#[extern(path = "fake-lib")]');
  expect(neoc).toContain("fn doThing(name: string) -> number;");
  expect(neoc).toContain('#[extern(path = "fake-lib:sub")]');
  expect(neoc).toContain("fn helper() -> bool;");

  // …followed by the ambient global surface, in the same file
  expect(neoc).toContain("#[extern(ambient)]\n    pub fn tfetch(url: string) -> TResponse;");
  expect(neoc).toContain('#[extern(ambient, path = "tconsole")]');

  // module surface precedes the globals
  expect(neoc.indexOf('path = "fake-lib"')).toBeLessThan(neoc.indexOf('path = "tconsole"'));
});

test("generateProject skips an undeclared specifier and reports what was found", () => {
  const { neoc, modules } = generateProject({ sourceFilePaths: [ambient] }, ["fake-lib", "no-such-module"]);
  expect(modules).toEqual(["fake-lib"]);
  expect(neoc).toContain('#[extern(path = "fake-lib")]');
  expect(neoc).not.toContain("no-such-module");
});
