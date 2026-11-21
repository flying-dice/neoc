/**
 * Apps as Luau (scripts/luau.ts, .darklua.json, lua/prelude.lua):
 *  - the sources the host embeds and the contract tests also run with a native Lua (`lua/prelude.lua`,
 *    `types/generated/ui.lua`) stay plain Lua source: no annotations, no Luau-only syntax;
 *  - every app bundles into one file whose only requires are runtime ones (`vos:*` and the host's `std:`, `lib:` and
 *    `vnd:` modules), with its types and comments stripped, its Luau syntax untouched and its non-ASCII text intact;
 *  - the prelude's `utf8` agrees with a native Lua 5.3+ when one is installed;
 *  - the bundles are loaded and run by the Rust tests (`crates/vos-lua/tests`, `crates/vos-host/tests`: the VM's
 *    Luau, on the bundle `scripts/luau-build.ts` makes). darklua is never skipped: without it these tests fail, as
 *    `build:apps` does.
 */
import { Glob } from "bun";
import { describe, expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { DARKLUA_VERSION, findDarklua, root, toLuau } from "./luau";

const apps = [...new Glob("apps/*/app.json").scanSync({ cwd: root })].sort();

function entryOf(manifest: string): string {
  const main =
    (
      JSON.parse(readFileSync(join(root, manifest), "utf8")) as {
        main?: string;
      }
    ).main ?? "app.luau";
  return join(root, dirname(manifest), main);
}

/** A source with its `--` comments dropped (good enough to scan: a `--` inside a string only hides text). */
function code(file: string): string {
  return readFileSync(join(root, file), "utf8")
    .split("\n")
    .map((line) => line.replace(/--.*$/, ""))
    .join("\n");
}

describe("the embedded Lua sources stay plain Lua", () => {
  for (const file of ["lua/prelude.lua", "types/generated/ui.lua"]) {
    test(`${file} has no annotations and no Luau-only syntax`, () => {
      const source = code(file);
      expect(
        source.match(
          /\+=|-=|\.\.=|\bcontinue\b|`|\bexport type\b|\)\s*::\s*\w|^\s*local function \w+\([^)]*\w:\s*\w/m,
        )?.[0],
      ).toBeUndefined();
      // `for k, v in tbl do` (generalized iteration) is Luau: write pairs(tbl) or ipairs(tbl)
      expect(
        source.match(/^\s*for\s+[\w, ]+\s+in\s+[\w.[\]]+\s+do\b/m)?.[0],
      ).toBeUndefined();
    });
  }
});

describe("the apps are Luau", () => {
  test("every app's entry is a .luau file that opts in to strict mode", () => {
    expect(apps.length).toBeGreaterThan(1);
    for (const manifest of apps) {
      const entry = entryOf(manifest);
      expect(entry).toEndWith(".luau");
      expect(readFileSync(entry, "utf8").startsWith("--!strict\n")).toBe(true);
    }
  });
});

/** What stays a `require` in a bundle: `vos:*` and the host's neoc modules, which the VM resolves at run time. */
const RUNTIME_MODULE = /^(vos:|std:|lib:|vnd:)/;

describe("darklua", () => {
  const darklua = findDarklua();
  test(`is installed (pinned: ${DARKLUA_VERSION})`, () => {
    expect(
      darklua,
      `install it: cargo install darklua --version ${DARKLUA_VERSION} --locked`,
    ).toBeDefined();
  });
  if (!darklua) return;

  for (const manifest of apps) {
    const dir = dirname(manifest);
    test(`${dir} bundles into one file that requires only runtime modules`, () => {
      const out = toLuau(darklua, entryOf(manifest));
      const required = [
        ...out.matchAll(/\brequire\s*\(\s*(['"])(.*?)\1\s*\)/g),
      ].map((m) => m[2]);
      expect(required.length).toBeGreaterThan(0);
      expect(
        required.filter((name) => !RUNTIME_MODULE.test(name ?? "")),
      ).toEqual([]);
      // the types are stripped (the host does not read them), the strict marker with the other comments
      expect(out).not.toMatch(/--!|\bexport type\b|\n\s*type \w+\s*=/);
    });
  }

  test("leaves the host modules (std:, lib:, vnd:) as runtime requires and bundles relative ones", () => {
    const scratch = mkdtempSync(join(tmpdir(), "vos-luau-modules-"));
    try {
      writeFileSync(
        join(scratch, "helper.luau"),
        "--!strict\nreturn { answer = 42 }\n",
      );
      writeFileSync(
        join(scratch, "app.luau"),
        '--!strict\nlocal fs = require("std:fs")\nlocal json = require("lib:json")\nlocal yaml = require("vnd:serde_yaml")\n' +
          'local app = require("vos:app")\nlocal helper = require("./helper")\nreturn { fs, json, yaml, app, helper }\n',
      );
      const out = toLuau(darklua, join(scratch, "app.luau"));
      for (const name of [
        "std:fs",
        "lib:json",
        "vnd:serde_yaml",
        "vos:app",
      ]) {
        expect(out).toContain(`require("${name}")`);
      }
      expect(out).not.toContain("./helper");
      expect(out).toContain("answer = 42");
    } finally {
      rmSync(scratch, { recursive: true, force: true });
    }
  });

  test("strips the types and keeps the Luau syntax the host runs", () => {
    const scratch = mkdtempSync(join(tmpdir(), "vos-luau-syntax-"));
    try {
      writeFileSync(
        join(scratch, "app.luau"),
        [
          "--!strict",
          "type Pair = { left: number, right: number }",
          "local function sum(pair: Pair): number",
          "  local total: number = 0",
          "  for _, value in { pair.left, pair.right } do",
          "    if value == 0 then",
          "      continue",
          "    end",
          "    total += value",
          "  end",
          "  return total",
          "end",
          'local label = if sum({ left = 1, right = 2 }) > 2 then "big" else "small"',
          "return `{label}` :: string",
          "",
        ].join("\n"),
      );
      const out = toLuau(darklua, join(scratch, "app.luau"));
      expect(out).not.toMatch(/type Pair|: number|: Pair|:: string/);
      for (const luau of ["continue", "total += value", "`{label}`", "if sum("]) {
        expect(out).toContain(luau);
      }
      // line numbers in an error from the VM point at the source (the `retain_lines` generator)
      expect(out.split("\n").findIndex((l) => l.includes("total += value"))).toBe(
        8,
      );
    } finally {
      rmSync(scratch, { recursive: true, force: true });
    }
  });

  test("keeps non-ASCII text as it is written (the VM reads raw UTF-8 bytes in a string)", () => {
    const out = toLuau(darklua, join(root, "apps/calculator/app.luau"));
    expect(out).toContain('"×"');
    expect(out).not.toMatch(/\\u\{/);
  });
});

/** A native Lua 5.3+ (`lua5.3`, `lua5.4`, ... or `lua`), for the polyfill comparison; `LUA53=<path>` overrides. */
function findLua53(): string | undefined {
  const candidates = [process.env.LUA53, "lua5.4", "lua5.3", "lua5.5", "lua"].filter(
    (c): c is string => !!c,
  );
  for (const candidate of candidates) {
    const run = spawnSync(candidate, ["-e", "io.write(_VERSION)"], {
      encoding: "utf8",
    });
    if (run.status === 0 && /^Lua 5\.([3-9])$/.test(run.stdout.trim()))
      return candidate;
  }
  return undefined;
}

const lua53 = findLua53();
const generated = (name: string): string => join(root, "types/generated", name);

describe("the prelude polyfills", () => {
  test.skipIf(!lua53 || !existsSync(generated("prelude.lua")))(
    `agree with the native utf8 of Lua 5.3+${lua53 ? "" : " [SKIPPED: no Lua 5.3+ on PATH]"}`,
    () => {
      const run = spawnSync(
        lua53 as string,
        [
          join(root, "scripts/prelude-polyfill-check.lua"),
          generated("prelude.lua"),
        ],
        {
          encoding: "utf8",
        },
      );
      expect(`${run.stdout}${run.stderr}`).toMatch(/^ok \d+ comparisons/);
      expect(run.status).toBe(0);
    },
  );
  if (!lua53)
    process.stderr.write(
      "\n!! polyfill comparison SKIPPED: no Lua 5.3+ interpreter on PATH\n",
    );
});
