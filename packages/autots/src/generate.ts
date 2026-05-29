import {
  Project,
  Node,
  ModuleDeclarationKind,
  ts,
  type SourceFile,
  type Type,
  type Symbol as TsSymbol,
  type ExportedDeclarations,
} from "ts-morph";
import * as path from "node:path";
import { createRequire } from "node:module";
import { isNeocIdent } from "./map-type.js";

export interface GenerateOptions {
  /** Obsolete: the model has no abi string (one target per compilation). Accepted but ignored. */
  host?: string;
  /** Base directory used to derive an entry source file's module path. Default `process.cwd()`. */
  cwd?: string;
  /** Ambient-module filters: exact (`"bun"`) or trailing-`*` prefix (`"bun:*"`). Empty = all (default). */
  modules?: string[];
  /** Emit the `declare global` surface as `#[extern(ambient)]` items. Default `true`. */
  globals?: boolean;
}

export interface GenerateInput {
  tsConfigFilePath?: string;
  sourceFilePaths?: string[];
}

type Entry = readonly [string, ExportedDeclarations];

/** Members never emitted as struct fields or methods. */
const MEMBER_SKIP = new Set(["constructor", "prototype"]);

/** Names the generator must not redefine (neoc built-ins / reserved). */
const RESERVED_NAMES = new Set<string>(["Unknown"]);

/** A foreign top-level name we can emit: a legal neoc identifier, not `default`, not reserved. */
function usableName(name: string): boolean {
  return isNeocIdent(name) && name !== "default" && !RESERVED_NAMES.has(name);
}

/** A struct/method member name we can emit. */
function usableMember(name: string): boolean {
  return isNeocIdent(name) && !MEMBER_SKIP.has(name) && !name.startsWith("__") && !RESERVED_NAMES.has(name);
}

/**
 * The emitted structs of a surface, mapped to their generic *arity* (type-param
 * count). A type reference to a key resolves to that struct, instantiated with
 * exactly `arity` arguments (`reduce`); everything else degrades to `Unknown`.
 */
type Structs = Map<string, number>;

/** Type-parameter names in scope at a position — used to resolve a bare `T`. */
type Scope = Set<string>;

/** A neoc-spellable type-parameter name: the TS convention, uppercase-leading. */
const TYPE_PARAM = /^[A-Z][A-Za-z0-9_]*$/;

/** A node that may carry generic parameters (interface/class/function/method). */
interface HasTypeParams {
  getTypeParameters?: () => { getName(): string }[];
}

/**
 * The generic parameter names of a declaration, or `[]` when it is not generic
 * — or carries any name neoc can't spell (then the declaration is treated as
 * non-generic so its arity stays consistent everywhere it is referenced).
 */
function typeParamNames(decl: HasTypeParams): string[] {
  const names = (decl.getTypeParameters?.() ?? []).map((tp) => tp.getName());
  if (names.length === 0) return [];
  if (!names.every((n) => TYPE_PARAM.test(n) && !RESERVED_NAMES.has(n))) return [];
  return names;
}

/** Render a `<A, B>` generic header, or `""` for a non-generic declaration. */
function genericHeader(params: string[]): string {
  return params.length > 0 ? `<${params.join(", ")}>` : "";
}

/** Derive a neoc module identifier from a module path: last `:`/`/` segment, non-ident chars → `_`. */
function modIdent(modPath: string): string {
  const seg = modPath.split(/[:/]/).pop() ?? modPath;
  let id = seg.replace(/[^A-Za-z0-9_]/g, "_");
  if (!/^[A-Za-z_]/.test(id)) id = `_${id}`;
  return id;
}

/**
 * Sanitize a full module specifier into a single neoc identifier, used for both
 * the generated file name and its `use`-path segment: `bun:sqlite` →
 * `bun_sqlite`, `node:fs` → `node_fs`. Every `:`/`-`/`/`/`.` becomes `_`; a
 * leading non-letter is prefixed with `_`.
 */
export function moduleSlug(modPath: string): string {
  let id = modPath.replace(/[^A-Za-z0-9_]/g, "_");
  if (!/^[A-Za-z_]/.test(id)) id = `_${id}`;
  return id;
}

const q = (s: string): string => JSON.stringify(s);

/** A module surface: its target path plus its exported declarations. */
interface Surface {
  path: string;
  entries: Entry[];
}

/** Load a ts-morph `Project` from the input and return it with its entry files. */
function loadProject(input: GenerateInput): { project: Project; entryFiles: SourceFile[] } {
  let project: Project;
  let entryFiles: SourceFile[];
  if (input.tsConfigFilePath) {
    project = new Project({ tsConfigFilePath: input.tsConfigFilePath });
    entryFiles = project.getSourceFiles();
  } else {
    project = new Project({ compilerOptions: { allowJs: true } });
    entryFiles = (input.sourceFilePaths ?? []).map((p) => project.addSourceFileAtPath(p));
  }
  addReferencedTypePackages(project, entryFiles);
  project.resolveSourceFileDependencies();
  return { project, entryFiles };
}

/**
 * Generate the `.neoc` file for a single ambient `declare module "<specifier>"`.
 *
 * The file declares one top-level `pub` surface — every exported declaration of
 * the module, emitted directly inside the file (an enclosing extern `mod` carries
 * the `#[extern(path = "<specifier>")]` so the native import is reconstructed in
 * the consumer). Consumers `use autots::<slug>::Item`, where `<slug>` is the
 * sanitized specifier ([`moduleSlug`]). Returns `null` if the specifier names no
 * declared module in the loaded sources.
 */
export function generateModule(input: GenerateInput, specifier: string): string | null {
  const { project } = loadProject(input);
  const found = ambientModules(project.getSourceFiles(), [specifier]).find(
    ([name]) => name === specifier,
  );
  if (!found) return null;
  const [, entries] = found;
  const block = emitModule({ path: specifier, entries });
  return render(block.length > 0 ? [block] : []);
}

/**
 * Generate the `autots/globals.neoc` file: the `declare global` surface of the
 * loaded sources, emitted as `#[extern(ambient)]` items. Returns the rendered
 * file (an empty `extern { }` when the sources declare no globals).
 */
export function generateGlobals(input: GenerateInput): string {
  const { project } = loadProject(input);
  const ambient = emitGlobalSurface(globalEntries(project));
  return render(ambient.length > 0 ? [ambient] : []);
}

/** The single-file project surface plus the specifiers actually found. */
export interface ProjectSurface {
  /** The rendered `types.neoc`: one `mod` per found specifier, then the globals. */
  neoc: string;
  /** Configured specifiers that resolved to an ambient module, in config order. */
  modules: string[];
}

/**
 * Generate the project's whole interop surface as a single file: one
 * `#[extern(path = "<specifier>")] mod` block per configured `specifiers`
 * entry (in the order given), followed by the global surface as
 * `#[extern(ambient)]` items. This is what the convention-driven CLI writes to
 * `src/main/neoc/autots/types.neoc` — consumers `use autots::types::Item`.
 *
 * A specifier that names no ambient `declare module` in the loaded sources is
 * skipped; the returned `modules` lists those that were found.
 */
export function generateProject(input: GenerateInput, specifiers: string[]): ProjectSurface {
  const { project } = loadProject(input);
  const byName = new Map(ambientModules(project.getSourceFiles(), specifiers));

  const blocks: string[] = [];
  const modules: string[] = [];
  for (const spec of specifiers) {
    const entries = byName.get(spec);
    if (!entries) continue;
    const block = emitModule({ path: spec, entries });
    if (block.length === 0) continue;
    blocks.push(block);
    modules.push(spec);
  }

  // Pull in the whole TS world reachable from the configured packages — the
  // standard library and every transitive dependency, not just the package's
  // own files.
  const ambient = emitGlobalSurface(globalEntries(project, /* wholeProgram */ true));
  if (ambient.length > 0) blocks.push(ambient);

  return { neoc: render(blocks), modules };
}

/** Generate a `.neoc` `extern { }` block for a TypeScript surface. */
export function generate(input: GenerateInput, opts: GenerateOptions = {}): string {
  const cwd = opts.cwd ?? process.cwd();
  const modules = opts.modules ?? [];
  const emitGlobals = opts.globals ?? true;
  void opts.host; // obsolete: no abi string in the model

  const { project, entryFiles } = loadProject(input);

  const surfaces: Surface[] = [];
  for (const sf of entryFiles) {
    const entries: Entry[] = [];
    for (const [name, decls] of sf.getExportedDeclarations()) {
      const d = decls[0];
      if (d) entries.push([name, d]);
    }
    if (entries.length > 0) surfaces.push({ path: linkName(sf, cwd), entries });
  }
  for (const [name, entries] of ambientModules(project.getSourceFiles(), modules)) {
    surfaces.push({ path: name, entries });
  }

  const blocks = surfaces.map((s) => emitModule(s)).filter((m) => m.length > 0);
  if (emitGlobals) {
    const ambient = emitGlobalSurface(globalEntries(project));
    if (ambient.length > 0) blocks.push(ambient);
  }
  return render(blocks);
}

/** Render one `#[extern(path = "…")] mod <id> { … }` block. Empty array if nothing emitted. */
function emitModule(surface: Surface): string {
  // The struct names this module emits (interfaces + classes with usable names),
  // each with its generic arity. A type ref to one resolves to that struct
  // instantiated to its arity; everything else degrades to `Unknown`.
  // TODO: clean-code - 0.6 - DRY: this struct-arity collection loop is duplicated verbatim in emitGlobalSurface (~L630, over `entries`); extract a `collectStructs(entries: Entry[]): Structs` helper.
  const structs: Structs = new Map();
  for (const [name, decl] of surface.entries) {
    if (!usableName(name) || structs.has(name)) continue;
    if (Node.isInterfaceDeclaration(decl) || Node.isClassDeclaration(decl))
      structs.set(name, typeParamNames(decl).length);
  }

  const items: string[] = [];
  const emittedStructs = new Set<string>();
  const emittedFns = new Set<string>();

  for (const [name, decl] of surface.entries) {
    if (!usableName(name)) continue;
    try {
      if (Node.isInterfaceDeclaration(decl) || Node.isClassDeclaration(decl)) {
        if (emittedStructs.has(name)) continue;
        emittedStructs.add(name);
        items.push(...emitStruct(name, decl, structs));
      } else if (Node.isFunctionDeclaration(decl)) {
        if (emittedFns.has(name)) continue;
        emittedFns.add(name);
        items.push(emitFreeFn(name, decl, structs));
      }
      // Skipped (v1): variable/const exports, type-alias object types, generics.
    } catch {
      /* skip a declaration ts-morph can't resolve */
    }
  }

  if (items.length === 0) return "";
  const body = items.map((x) => indent(x, "        ")).join("\n");
  return `    #[extern(path = ${q(surface.path)})]\n    mod ${modIdent(surface.path)} {\n${body}\n    }`;
}

/** Indent every line of `block` with `pad`. */
function indent(block: string, pad: string): string {
  return block
    .split("\n")
    .map((l) => (l.length > 0 ? pad + l : l))
    .join("\n");
}

/** Emit a `struct` (with fields) plus, if it has methods/ctor, an `impl`. Returns the items. */
function emitStruct(name: string, decl: ExportedDeclarations, structs: Structs): string[] {
  const isClass = Node.isClassDeclaration(decl);
  const objType = decl.getType();

  // The struct's own type parameters are in scope for every field and method.
  // `decl` is always an interface/class here (the only callers), both generic-capable.
  const scope: Scope = new Set(typeParamNames(decl as HasTypeParams));
  const header = genericHeader([...scope]);

  const fields: string[] = [];
  const methods: string[] = [];
  const ctorLine = isClass ? constructorLine(decl, structs, scope) : null;
  const seen = new Set<string>();

  let props: TsSymbol[];
  try {
    props = objType.getProperties();
  } catch {
    props = [];
  }
  for (const prop of props) {
    try {
      const m = prop.getName();
      if (seen.has(m) || !usableMember(m)) continue;
      const at = prop.getDeclarations()[0] ?? decl;
      const pt = prop.getTypeAtLocation(at);
      const callSig = pt.getNonNullableType().getCallSignatures()[0];

      if (callSig && !isFunctionTypeProperty(prop)) {
        // A genuine method (declared as `m(): T`) → impl fn. A method may add its
        // own type parameters, layered over the struct's.
        seen.add(m);
        const isStatic = isStaticMember(prop);
        const own = Node.isMethodSignature(at) || Node.isMethodDeclaration(at) ? typeParamNames(at) : [];
        methods.push(emitMethod(m, callSig, structs, isStatic, scope, own));
      } else {
        // A data property, or a function-typed property (callback) → struct field.
        seen.add(m);
        const ty = fieldType(pt, structs, scope);
        fields.push(`${m}: ${ty}`);
      }
    } catch {
      continue; // skip a property ts-morph can't resolve
    }
  }

  const out: string[] = [];
  out.push(fields.length > 0 ? `pub struct ${name}${header} { ${fields.join(", ")} }` : `pub struct ${name}${header};`);

  const implBody: string[] = [];
  if (ctorLine) implBody.push(ctorLine);
  implBody.push(...methods);
  if (implBody.length > 0) {
    out.push(`impl ${name}${header} {\n${implBody.map((m) => indent(m, "    ")).join("\n")}\n}`);
  }
  return out;
}

/** Whether a property symbol is declared `static`. */
function isStaticMember(prop: TsSymbol): boolean {
  return prop.getDeclarations().some((d) => {
    const ms = (d as unknown as { isStatic?: () => boolean }).isStatic;
    return typeof ms === "function" ? ms.call(d) : false;
  });
}

/**
 * Whether a property is a *function-typed property* (a callback field like
 * `fetch: (req) => Response`) rather than a method (`m(): T`). A method's
 * declaration is a `MethodSignature`/`MethodDeclaration`; a function-typed field
 * is a property whose value happens to be a function type.
 */
function isFunctionTypeProperty(prop: TsSymbol): boolean {
  return prop.getDeclarations().some((d) => Node.isPropertySignature(d) || Node.isPropertyDeclaration(d));
}

/**
 * Emit a bodyless impl method. Instance → `fn m(self, …) -> T;`; static →
 * `fn m(…) -> T;`. `structScope` is the enclosing struct's type parameters;
 * `own` are the method's own, layered on top and printed in its `<…>` header.
 */
function emitMethod(
  name: string,
  sig: import("ts-morph").Signature,
  structs: Structs,
  isStatic: boolean,
  structScope: Scope,
  own: string[],
): string {
  const scope: Scope = own.length > 0 ? new Set([...structScope, ...own]) : structScope;
  const params = sigParamList(sig.getParameters(), structs, scope);
  const head = isStatic ? params : ["self", ...params];
  const ret = retType(sig.getReturnType(), structs, scope);
  return `pub fn ${name}${genericHeader(own)}(${head.join(", ")})${ret ? ` -> ${ret}` : ""};`;
}

/**
 * Reduce a call-signature's parameter *symbols* to `name: T` strings, dropping
 * trailing optional and rest params (a parameter symbol's optionality lives on
 * its declaration).
 */
function sigParamList(syms: TsSymbol[], structs: Structs, scope: Scope): string[] {
  const isRest = (s: TsSymbol): boolean =>
    s.getDeclarations().some((d) => Node.isParameterDeclaration(d) && d.isRestParameter());
  const isOpt = (s: TsSymbol): boolean =>
    s.getDeclarations().some((d) => Node.isParameterDeclaration(d) && d.isOptional());

  let end = syms.length;
  while (end > 0 && isRest(syms[end - 1]!)) end--;
  while (end > 0 && isOpt(syms[end - 1]!)) end--;

  const out: string[] = [];
  for (let i = 0; i < end; i++) {
    const s = syms[i]!;
    const pn = isNeocIdent(s.getName()) ? s.getName() : `arg${i}`;
    const d = s.getDeclarations()[0];
    const pt = d ? s.getTypeAtLocation(d) : undefined;
    out.push(`${pn}: ${pt ? paramType(pt, structs, scope) : "Unknown"}`);
  }
  return out;
}

/** Emit a bodyless free function, printing its own generic parameters. */
function emitFreeFn(name: string, decl: import("ts-morph").FunctionDeclaration, structs: Structs): string {
  const scope: Scope = new Set(typeParamNames(decl));
  const params = paramList(decl.getParameters(), structs, scope);
  const ret = retType(decl.getReturnType(), structs, scope);
  return `pub fn ${name}${genericHeader([...scope])}(${params.join(", ")})${ret ? ` -> ${ret}` : ""};`;
}

/**
 * Build a constructor line for a class. `#[extern(constructor)] fn new(…);` —
 * no return type. Trailing optional and rest params dropped; if every param is
 * optional, keep the first. The class's type parameters are in `scope`.
 */
function constructorLine(decl: ExportedDeclarations, structs: Structs, scope: Scope): string {
  const ctor = Node.isClassDeclaration(decl) ? decl.getConstructors()[0] : undefined;
  const all = ctor?.getParameters() ?? [];
  const params = paramList(all, structs, scope, /* keepFirstIfAllOptional */ true);
  return `#[extern(constructor)]\npub fn new(${params.join(", ")});`;
}

/**
 * Reduce a parameter list to `name: T` strings, dropping trailing optional and
 * rest params. When `keepFirstIfAllOptional` and every param is optional, keep
 * the first.
 */
function paramList(
  ps: import("ts-morph").ParameterDeclaration[],
  structs: Structs,
  scope: Scope,
  keepFirstIfAllOptional = false,
): string[] {
  // Drop trailing rest params, then trailing optional params.
  let end = ps.length;
  while (end > 0 && ps[end - 1]!.isRestParameter()) end--;
  let optEnd = end;
  while (optEnd > 0 && ps[optEnd - 1]!.isOptional()) optEnd--;
  if (keepFirstIfAllOptional && optEnd === 0 && end > 0) optEnd = 1;
  end = optEnd;

  const out: string[] = [];
  for (let i = 0; i < end; i++) {
    const p = ps[i]!;
    const pn = isNeocIdent(p.getName()) ? p.getName() : `arg${i}`;
    out.push(`${pn}: ${paramType(p.getType(), structs, scope)}`);
  }
  return out;
}

/** A struct field type: non-null base; a unit base is `Unknown`. */
function fieldType(t: Type, structs: Structs, scope: Scope): string {
  const r = reduce(t, structs, scope);
  return r === "()" ? "Unknown" : r;
}

/** A parameter type: a unit param becomes `Unknown` (never `()`). */
function paramType(t: Type, structs: Structs, scope: Scope): string {
  const r = reduce(t, structs, scope);
  return r === "()" ? "Unknown" : r;
}

/** A return type, or `null` when unit (so the `-> …` is omitted). */
function retType(t: Type, structs: Structs, scope: Scope): string | null {
  const r = reduce(t, structs, scope);
  return r === "()" ? null : r;
}

/**
 * Reduce a TS type to a neoc type, degrading toward the bare value. Returns
 * `"()"` for unit (caller decides omission/`Unknown`). `scope` holds the
 * type-parameter names in scope, so a bare `T` resolves to itself when it is a
 * declared parameter and to `Unknown` otherwise.
 */
function reduce(t: Type, structs: Structs, scope: Scope): string {
  // Check unit and type-parameter-ness on the *original* type:
  // `getNonNullableType()` resolves a type parameter to its constraint, erasing
  // its identity (and strips the void-ness this first check needs).
  if (t.isVoid() || t.isUndefined() || t.isNull()) return "()";

  // A type parameter → its own name when in scope; otherwise the boundary type.
  if (t.isTypeParameter()) {
    const n = t.getSymbol()?.getName();
    return n !== undefined && scope.has(n) ? n : "Unknown";
  }

  // Primitives are read through the non-nullable type so an optional `boolean`
  // (which TS may expand to `true | false | undefined`) still folds to `bool`.
  const nn = t.getNonNullableType();
  if (nn.isString() || nn.isStringLiteral()) return "string";
  if (nn.isNumber() || nn.isNumberLiteral()) return "number";
  if (nn.isBoolean() || nn.isBooleanLiteral()) return "bool";

  // Everything below reads the *original* `t`: `getNonNullableType()` resolves
  // type parameters to their constraint and erases type arguments, so it can't
  // be used where identity or generic args must survive.

  // A union of string literals is `string`; `T | undefined` (a single residual
  // after dropping null/undefined) is that residual; any other union → Unknown.
  if (t.isUnion()) {
    const parts = t.getUnionTypes().filter((u) => !u.isUndefined() && !u.isNull());
    if (parts.length > 0 && parts.every((u) => u.isStringLiteral() || u.isString())) return "string";
    if (parts.length === 1) return reduce(parts[0]!, structs, scope);
    return "Unknown";
  }

  // Array `T[]` / `Array<T>` → neoc list `[T]`.
  if (t.isArray()) {
    const el = t.getArrayElementType();
    const elTy = el ? reduce(el, structs, scope) : "Unknown";
    return `[${elTy === "()" ? "Unknown" : elTy}]`;
  }

  // A pure function type → `fn(A) -> B`.
  const callSigs = t.getCallSignatures();
  if (callSigs.length > 0 && t.getProperties().length === 0) {
    const sig = callSigs[0]!;
    const params = sig
      .getParameters()
      .map((p) => {
        const d = p.getDeclarations()[0];
        const pt = d ? p.getTypeAtLocation(d) : undefined;
        const r = pt ? reduce(pt, structs, scope) : "Unknown";
        return r === "()" ? "Unknown" : r;
      });
    const ret = reduce(sig.getReturnType(), structs, scope);
    return `fn(${params.join(", ")})${ret === "()" ? "" : ` -> ${ret}`}`;
  }

  // A known emitted struct → its name, instantiated with exactly its declared
  // arity of type arguments (missing ones padded with `Unknown`, so the count
  // always matches the struct's `<…>` header — never an arity mismatch).
  const symName = t.getSymbol()?.getName() ?? t.getAliasSymbol()?.getName();
  if (symName !== undefined && structs.has(symName)) {
    const arity = structs.get(symName)!;
    if (arity === 0) return symName;
    const args = t.getTypeArguments();
    const parts: string[] = [];
    for (let i = 0; i < arity; i++) {
      const r = args[i] ? reduce(args[i]!, structs, scope) : "Unknown";
      parts.push(r === "()" ? "Unknown" : r);
    }
    return `${symName}<${parts.join(", ")}>`;
  }

  // Everything else: any/unknown/Object, intersection, unresolved → Unknown.
  return "Unknown";
}

function render(blocks: string[]): string {
  const header = `// @generated by autots — do not edit.\n`;
  if (blocks.length === 0) return `${header}extern {\n}\n`;
  return `${header}extern {\n${blocks.join("\n\n")}\n}\n`;
}

/**
 * Collect the global surface, merging across files. Each entry is a top-level
 * global name → its first declaration. Two sources contribute, both following
 * TypeScript's own notion of "global":
 *
 * 1. `declare global { … }` blocks in any file (a *module* file augmenting the
 *    global scope — e.g. `bun-types/bun.ns.d.ts`, which is how `Bun` is bound).
 * 2. Top-level ambient declarations of a *global script* file — a `.d.ts` with
 *    no top-level `import`/`export`, so its declarations live in the global
 *    scope directly. This is where runtime globals like `console`, `fetch`, and
 *    `setTimeout` are declared (e.g. `bun-types/globals.d.ts`).
 *
 * `wholeProgram` decides how wide "any file" is:
 *
 * - `false` (default) — only the files explicitly added to the project (the
 *   configured types packages and what `addReferencedTypePackages` pulled in).
 * - `true` — every file the TypeScript compiler resolved while loading those
 *   packages: the standard library (`lib.es*`, `lib.dom`: `Math`, `JSON`,
 *   `Promise`, `Date`, `Map`, `Set`, …) and all transitive dependencies. This
 *   is the whole TS world reachable from the package. Each such file is wrapped
 *   into the project so it can be walked.
 *
 * A `declare global` binding wins over a top-level one of the same name.
 */
function globalEntries(project: Project, wholeProgram = false): Entry[] {
  // TODO: clean-code - 0.55 - COUPLING: this read-shaped collector, when wholeProgram is true, mutates the shared Project (addSourceFileAtPathIfExists) that generateProject already used for ambientModules — a hidden side effect + order-dependence (modules must be harvested before globals, else ambientModules sees stdlib/dep declare-module blocks). Lift the project-expansion out of the collector into its own step.
  if (wholeProgram) {
    const program = project.getProgram().compilerObject;
    for (const sf of program.getSourceFiles()) project.addSourceFileAtPathIfExists(sf.fileName);
  }

  const merged = new Map<string, ExportedDeclarations>();
  const add = (name: string, decl: ExportedDeclarations | undefined): void => {
    if (decl && name && !merged.has(name)) merged.set(name, decl);
  };

  const files = project.getSourceFiles();

  // (1) `declare global { … }` augmentation blocks, in any file.
  for (const sf of files) {
    for (const mod of sf.getModules()) {
      if (mod.getDeclarationKind() !== ModuleDeclarationKind.Global) continue;
      for (const [name, decls] of mod.getExportedDeclarations()) add(name, decls[0]);
    }
  }

  // (2) Top-level declarations of global-script files (no top-level import/export).
  for (const sf of files) {
    if (ts.isExternalModule(sf.compilerNode)) continue; // a module — top-level decls aren't global
    for (const stmt of sf.getStatements()) {
      if (Node.isFunctionDeclaration(stmt) || Node.isInterfaceDeclaration(stmt) || Node.isClassDeclaration(stmt)) {
        add(stmt.getName() ?? "", stmt);
      } else if (Node.isVariableStatement(stmt)) {
        for (const v of stmt.getDeclarations()) add(v.getName(), v);
      } else if (Node.isModuleDeclaration(stmt) && stmt.getDeclarationKind() === ModuleDeclarationKind.Namespace) {
        // `declare namespace Reflect / Intl / WebAssembly { … }` — a global
        // namespace object reached by name.
        add(stmt.getName(), stmt);
      }
    }
  }

  return [...merged.entries()].sort((a, b) => a[0].localeCompare(b[0]));
}

/**
 * Emit the `declare global` surface as `#[extern(ambient)]` items. Functions,
 * classes, and interfaces become ambient `fn`/`struct`(+`impl`); a namespace
 * object var (a var whose type carries callable members, e.g. `Bun`) becomes an
 * `#[extern(ambient, path = "<VarName>")] mod`. Returns the joined item block, or
 * `""` when nothing is emitted.
 */
function emitGlobalSurface(entries: Entry[]): string {
  // Struct names this surface emits (interfaces + classes), each with its arity.
  const structs: Structs = new Map();
  for (const [name, decl] of entries) {
    if (!usableName(name) || structs.has(name)) continue;
    if (Node.isInterfaceDeclaration(decl) || Node.isClassDeclaration(decl))
      structs.set(name, typeParamNames(decl).length);
  }

  const items: string[] = [];
  const emitted = new Set<string>();

  for (const [name, decl] of entries) {
    if (!usableName(name) || emitted.has(name)) continue;
    try {
      if (Node.isInterfaceDeclaration(decl) || Node.isClassDeclaration(decl)) {
        emitted.add(name);
        // Reuse the module struct/impl emitter, then prefix the struct with the
        // ambient attribute. The `impl` carries no attribute — ambient propagates.
        const parts = emitStruct(name, decl, structs);
        parts[0] = `#[extern(ambient)]\n${parts[0]}`;
        items.push(...parts);
      } else if (Node.isFunctionDeclaration(decl)) {
        emitted.add(name);
        items.push(`#[extern(ambient)]\n${emitFreeFn(name, decl, structs)}`);
      } else if (Node.isVariableDeclaration(decl) || Node.isImportEqualsDeclaration(decl)) {
        const mod = emitGlobalNamespaceVar(name, decl, structs);
        if (mod) {
          emitted.add(name);
          items.push(mod);
        }
        // else: a primitive/simple global var — skipped (no clean ambient item).
      } else if (Node.isModuleDeclaration(decl)) {
        const mod = emitGlobalNamespace(name, decl, structs);
        if (mod) {
          emitted.add(name);
          items.push(mod);
        }
        // else: a namespace with no callable members — skipped.
      }
    } catch {
      /* skip a declaration ts-morph can't resolve */
    }
  }

  if (items.length === 0) return "";
  return items.map((x) => indent(x, "    ")).join("\n\n");
}

/**
 * Emit a namespace-object global var (e.g. `var Bun: …`) as an
 * `#[extern(ambient, path = "<VarName>")] mod`. The var's type's *method* members
 * become bodyless `fn`s. Returns `null` for a primitive/simple var (no callable
 * members) — those are skipped.
 */
function emitGlobalNamespaceVar(
  name: string,
  decl: ExportedDeclarations,
  structs: Structs,
): string | null {
  const t = decl.getType();
  let props: TsSymbol[];
  try {
    props = t.getProperties();
  } catch {
    return null;
  }

  const methods: string[] = [];
  const seen = new Set<string>();
  for (const prop of props) {
    try {
      const m = prop.getName();
      if (seen.has(m) || !usableMember(m)) continue;
      const at = prop.getDeclarations()[0];
      if (!at) continue;
      const pt = prop.getTypeAtLocation(at);
      const callSig = pt.getNonNullableType().getCallSignatures()[0];
      if (!callSig || isFunctionTypeProperty(prop)) continue; // skip data/callback props
      seen.add(m);
      // A self-less namespace fn; no enclosing struct generics, only its own.
      const own = Node.isMethodSignature(at) || Node.isMethodDeclaration(at) ? typeParamNames(at) : [];
      methods.push(emitMethod(m, callSig, structs, /* isStatic */ true, new Set(), own));
    } catch {
      continue;
    }
  }

  if (methods.length === 0) return null; // not a namespace object — skip.
  const body = methods.map((m) => indent(m, "    ")).join("\n");
  return `#[extern(ambient, path = ${q(name)})]\nmod ${modIdent(name)} {\n${body}\n}`;
}

// TODO: clean-code - 0.6 - NAMING: emitGlobalNamespace vs emitGlobalNamespaceVar differ only by a `Var` suffix and both emit a `mod`, but neither name conveys the input distinction (a TS `declare namespace` block vs a namespace-object value binding); rename to e.g. emitNamespaceDecl vs emitNamespaceObjectVar.
/**
 * Emit a global *namespace* (`declare namespace Reflect { … }`) as an
 * `#[extern(ambient, path = "<Name>")] mod`. Its exported functions become
 * bodyless `fn`s (overloads collapse to the first). Returns `null` for a
 * namespace with no callable functions (e.g. one that only nests types/classes).
 */
function emitGlobalNamespace(
  name: string,
  decl: import("ts-morph").ModuleDeclaration,
  structs: Structs,
): string | null {
  const fns: string[] = [];
  const seen = new Set<string>();
  for (const fn of decl.getFunctions()) {
    const fname = fn.getName();
    if (!fname || seen.has(fname) || !usableMember(fname)) continue;
    seen.add(fname);
    try {
      fns.push(emitFreeFn(fname, fn, structs));
    } catch {
      continue; // skip a function ts-morph can't resolve
    }
  }
  if (fns.length === 0) return null;
  const body = fns.map((m) => indent(m, "    ")).join("\n");
  return `#[extern(ambient, path = ${q(name)})]\nmod ${modIdent(name)} {\n${body}\n}`;
}

/** Collect ambient `declare module "name"` blocks, merging declarations that span files. */
function ambientModules(files: SourceFile[], filters: string[]): Array<[string, Entry[]]> {
  const merged = new Map<string, Map<string, ExportedDeclarations>>();
  for (const sf of files) {
    for (const mod of sf.getModules()) {
      if (mod.getDeclarationKind() !== ModuleDeclarationKind.Module) continue;
      const name = mod.getName().replace(/^["']|["']$/g, "");
      if (!moduleMatches(name, filters)) continue;
      const bucket = merged.get(name) ?? new Map<string, ExportedDeclarations>();
      for (const [declName, decls] of mod.getExportedDeclarations()) {
        const d = decls[0];
        if (d && !bucket.has(declName)) bucket.set(declName, d);
      }
      merged.set(name, bucket);
    }
  }
  return [...merged.entries()]
    .sort((a, b) => a[0].localeCompare(b[0]))
    .map(([name, m]) => [name, [...m.entries()].sort((a, b) => a[0].localeCompare(b[0]))]);
}

function moduleMatches(name: string, filters: string[]): boolean {
  if (filters.length === 0) return true;
  return filters.some((f) => (f.endsWith("*") ? name.startsWith(f.slice(0, -1)) : name === f));
}

/** Load the `.d.ts` of any package named in a `/// <reference types="x" />` of an entry file. */
function addReferencedTypePackages(project: Project, entryFiles: SourceFile[]): void {
  const refRe = /\/\/\/\s*<reference\s+types\s*=\s*["']([^"']+)["']\s*\/>/g;
  for (const sf of entryFiles) {
    const text = sf.getFullText();
    for (let m = refRe.exec(text); m; m = refRe.exec(text)) {
      const pkg = m[1];
      if (!pkg) continue;
      try {
        const pkgJson = createRequire(sf.getFilePath()).resolve(`${pkg}/package.json`);
        project.addSourceFilesAtPaths(`${path.dirname(pkgJson)}/**/*.d.ts`);
      } catch {
        // Unresolved type reference — skip; whatever already loaded still emits.
      }
    }
    refRe.lastIndex = 0;
  }
}

function linkName(sf: SourceFile, cwd: string): string {
  const rel = path.relative(cwd, sf.getFilePath());
  return rel.replace(/\.(d\.ts|tsx?|jsx?)$/, "").replace(/\\/g, "/");
}
