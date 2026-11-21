/**
 * Apps as plain Lua 5.1 (scripts/lua51.ts, .darklua.json, lua/prelude.lua):
 *  - always: the sources stay plain Lua 5.1 (no Luau syntax) and use only what Lua 5.1 and the prelude provide,
 *    every app bundles into one file whose only requires are runtime ones (`@agentos/*` and the host's `std:`, `lib:` and
 *    `vnd:` modules), non-ASCII text survives the bundle
 *    untouched, and the polyfills agree with a native Lua 5.3+ when one is installed;
 *  - the bundles are loaded and run by the Rust tests (`crates/vos-lua/tests`, `crates/vos-host/tests`:
 *    neoc-vm's Luau, on the bundle `scripts/lua51-build.ts` makes), so no system Lua 5.1 is needed here.
 *    darklua is never skipped: without it these tests fail, as `build:apps` does.
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
import { DARKLUA_VERSION, findDarklua, root, toLua51 } from "./lua51";

const SKIP_DIRS = [
  "node_modules/",
  "target/",
  ".claude/",
  "/tests/",
  "/e2e/",
  "/negative/",
  "dist/",
];

function luaSources(patterns: string[]): string[] {
  const found: string[] = [];
  for (const pattern of patterns) {
    for (const file of new Glob(pattern).scanSync({ cwd: root })) {
      if (!SKIP_DIRS.some((dir) => `/${file}`.includes(dir))) found.push(file);
    }
  }
  return found.sort();
}

const shipped = luaSources([
  "apps/**/*.lua",
  "lua/*.lua",
  "types/generated/ui.lua",
]);
const apps = [...new Glob("apps/*/app.json").scanSync({ cwd: root })].sort();

function entryOf(manifest: string): string {
  const main =
    (
      JSON.parse(readFileSync(join(root, manifest), "utf8")) as {
        main?: string;
      }
    ).main ?? "app.lua";
  return join(root, dirname(manifest), main);
}

/** A source with its `--` comments dropped (good enough to scan: a `--` inside a string only hides text). */
function code(file: string): string {
  return readFileSync(join(root, file), "utf8")
    .split("\n")
    .map((line) => line.replace(/--.*$/, ""))
    .join("\n");
}

describe("Lua sources stay plain Lua 5.1", () => {
  test("there are sources to check", () => {
    expect(shipped.length).toBeGreaterThan(3);
  });

  for (const file of shipped) {
    test(`${file} avoids Luau syntax and library members Lua 5.1 and the prelude lack`, () => {
      const source = code(file);
      // Luau-only syntax: compound assignment, `continue`, interpolated strings, type casts, type declarations
      expect(
        source.match(
          /\+=|-=|\.\.=|\bcontinue\b|`|\bexport type\b|\)\s*::\s*\w/,
        )?.[0],
      ).toBeUndefined();
      // `for k, v in tbl do` (generalized iteration) is Luau: write pairs(tbl) or ipairs(tbl)
      expect(
        source.match(/^\s*for\s+[\w, ]+\s+in\s+[\w.[\]]+\s+do\b/m)?.[0],
      ).toBeUndefined();
      // Library members neither Lua 5.1 nor the prelude provide
      const unprovided =
        /\b(?:table\.(?:find|freeze|isfrozen|create|clear)|math\.(?:clamp|round|sign|noise|lerp)|string\.split|bit32\.\w+|buffer\.\w+|task\.(?:spawn|wait|defer|delay))\b/;
      expect(source.match(unprovided)?.[0]).toBeUndefined();
    });
  }
});

/** What stays a `require` in a bundle: `@agentos/*` and the host's neoc modules, which the VM resolves at run time. */
const RUNTIME_MODULE = /^(@agentos\/|std:|lib:|vnd:)/;

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
      const out = toLua51(darklua, entryOf(manifest));
      const required = [
        ...out.matchAll(/\brequire\s*\(\s*(['"])(.*?)\1\s*\)/g),
      ].map((m) => m[2]);
      expect(required.length).toBeGreaterThan(0);
      expect(
        required.filter((name) => !RUNTIME_MODULE.test(name ?? "")),
      ).toEqual([]);
      // darklua's own bundle prelude must not leave Luau syntax behind
      expect(out).not.toMatch(/::|\\u\{/);
    });
  }

  test("leaves the host modules (std:, lib:, vnd:) as runtime requires and bundles relative ones", () => {
    const scratch = mkdtempSync(join(tmpdir(), "agentos-lua51-modules-"));
    try {
      writeFileSync(join(scratch, "helper.lua"), "return { answer = 42 }\n");
      writeFileSync(
        join(scratch, "app.lua"),
        'local fs = require("std:fs")\nlocal json = require("lib:json")\nlocal yaml = require("vnd:serde_yaml")\n' +
          'local app = require("@agentos/app")\nlocal helper = require("./helper")\nreturn { fs, json, yaml, app, helper }\n',
      );
      const out = toLua51(darklua, join(scratch, "app.lua"));
      for (const name of [
        "std:fs",
        "lib:json",
        "vnd:serde_yaml",
        "@agentos/app",
      ]) {
        expect(out).toContain(`require("${name}")`);
      }
      expect(out).not.toContain("./helper");
      expect(out).toContain("answer = 42");
    } finally {
      rmSync(scratch, { recursive: true, force: true });
    }
  });

  test("keeps non-ASCII text as it is written (Lua 5.1 reads raw UTF-8 bytes in a string)", () => {
    const out = toLua51(darklua, join(root, "apps/calculator/app.lua"));
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
    `agree with the native utf8, table.move and friends of Lua 5.3+${lua53 ? "" : " [SKIPPED: no Lua 5.3+ on PATH]"}`,
    () => {
      const run = spawnSync(
        lua53 as string,
        [
          join(root, "scripts/lua51-polyfill-check.lua"),
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
