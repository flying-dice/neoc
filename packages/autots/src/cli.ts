#!/usr/bin/env bun
import * as fs from "node:fs";
import * as path from "node:path";
import { createRequire } from "node:module";
import { generateProject } from "./generate.js";

/** One configured module under `[autots]`: the JS specifier and its types package. */
interface ModuleConfig {
  /** The JS import specifier — the ambient `declare module "<module>"` to extract. */
  module: string;
  /** The npm package providing the declarations (its `.d.ts` entry is resolved). */
  types: string;
}

interface AutotsConfig {
  modules: ModuleConfig[];
}

/** Walk up from `dir` to the first ancestor containing a `neoc.toml`. */
function findProjectRoot(dir: string): string | null {
  let cur = path.resolve(dir);
  for (;;) {
    if (fs.existsSync(path.join(cur, "neoc.toml"))) return cur;
    const parent = path.dirname(cur);
    if (parent === cur) return null;
    cur = parent;
  }
}

/** Read `[autots]` from a project's `neoc.toml`. Empty config if the table is absent. */
async function readConfig(root: string): Promise<AutotsConfig> {
  const manifest = path.join(root, "neoc.toml");
  const mod = (await import(manifest)) as { default?: Record<string, unknown> };
  const table = (mod.default?.autots ?? {}) as { modules?: unknown };
  const raw = Array.isArray(table.modules) ? table.modules : [];
  const modules: ModuleConfig[] = [];
  for (const entry of raw) {
    const e = entry as { module?: unknown; types?: unknown };
    if (typeof e.module !== "string" || typeof e.types !== "string") {
      throw new Error(
        `neoc.toml [autots]: each module needs a string 'module' and 'types' (got ${JSON.stringify(entry)})`,
      );
    }
    modules.push({ module: e.module, types: e.types });
  }
  return { modules };
}

/**
 * Resolve a types package's declaration entry, walking up from the project root
 * through `node_modules`. Honors the package's `package.json` `types`/`typings`
 * field; defaults to `index.d.ts`.
 */
function resolveTypesEntry(typesPkg: string, root: string): string {
  const req = createRequire(path.join(root, "package.json"));
  let pkgJsonPath: string;
  try {
    pkgJsonPath = req.resolve(`${typesPkg}/package.json`);
  } catch {
    throw new Error(
      `could not find the types package '${typesPkg}' in any node_modules above ${root}. Is it installed?`,
    );
  }
  const pkgDir = path.dirname(pkgJsonPath);
  const pkg = JSON.parse(fs.readFileSync(pkgJsonPath, "utf8")) as {
    types?: string;
    typings?: string;
  };
  const entry = pkg.types ?? pkg.typings ?? "index.d.ts";
  return path.resolve(pkgDir, entry);
}

/** Write `contents` to `file`, creating parent directories, and log the path. */
function write(file: string, contents: string, root: string): void {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, contents);
  process.stderr.write(`autots: wrote ${path.relative(root, file)}\n`);
}

async function main(): Promise<void> {
  const root = findProjectRoot(process.cwd());
  if (!root) {
    process.stderr.write(
      "autots: no neoc.toml found. Run inside a project — autots reads the [autots] table " +
        "and writes extern modules to src/main/neoc/autots/.\n",
    );
    process.exit(1);
  }

  const config = await readConfig(root);
  if (config.modules.length === 0) {
    process.stderr.write(
      "autots: no [autots] modules configured in neoc.toml. Add e.g.\n" +
        '  [autots]\n  modules = [{ module = "bun:sqlite", types = "@types/bun" }]\n',
    );
    process.exit(1);
  }

  const outDir = path.join(root, "src", "main", "neoc", "autots");

  // Resolve every configured types package to its declaration entry (deduped),
  // and collect the module specifiers in config order. All of it — every
  // module's surface plus the shared global surface — lands in one file.
  const typeEntries = new Set<string>();
  const specifiers: string[] = [];
  for (const { module, types } of config.modules) {
    typeEntries.add(resolveTypesEntry(types, root));
    specifiers.push(module);
  }

  const { neoc, modules } = generateProject({ sourceFilePaths: [...typeEntries] }, specifiers);

  // Warn for any configured specifier the loaded types don't actually declare.
  for (const spec of specifiers) {
    if (!modules.includes(spec)) {
      process.stderr.write(`autots: module '${spec}' is not declared by its configured types. Skipping.\n`);
    }
  }

  write(path.join(outDir, "types.neoc"), neoc, root);
}

main().catch((e: unknown) => {
  process.stderr.write(`autots: ${e instanceof Error ? e.message : String(e)}\n`);
  process.exit(1);
});
