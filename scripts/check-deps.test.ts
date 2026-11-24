import { describe, expect, test } from "bun:test";
import { findProblems, type WorkspaceCrate } from "./check-deps";

/** The workspace as it should be: every crate in the table, each with only the edges it may have. */
function workspace(
  extra: Record<string, { name: string; kind: string | null }[]> = {},
  without: string[] = [],
): WorkspaceCrate[] {
  const edges: Record<string, string[]> = {
    "vos-domain": [],
    "vos-application": ["vos-domain"],
    "vos-lua": ["vos-domain", "vos-application", "neoc-vm"],
    "neoc-vm": ["neoc-vm-core", "neoc-lib-hex", "neoc-vnd-rusqlite"],
    "neoc-vm-core": ["mlua"],
    "neoc-lib-hex": ["neoc-vm-core", "mlua"],
    "neoc-vnd-rusqlite": ["neoc-vm-core", "mlua", "rusqlite"],
    "neoc-agui": [],
    "neoc-lib-agui": ["neoc-vm-core", "neoc-agui", "mlua"],
    neoc: ["neoc-vm", "mlua"],
    "vos-sqlite": ["vos-domain", "vos-application", "rusqlite"],
    "vos-fetch": ["vos-application"],
    "vos-mcp": ["vos-domain", "vos-application", "rmcp", "axum"],
    "vos-http": ["vos-domain", "vos-application", "axum"],
    "vos-codegen": ["vos-domain", "vos-lua"],
    "vos-host": [
      "vos-domain",
      "vos-application",
      "vos-lua",
      "vos-sqlite",
      "vos-fetch",
      "vos-mcp",
      "vos-http",
      "axum",
      "axum-server",
    ],
  };
  return Object.entries(edges)
    .filter(([name]) => !without.includes(name))
    .map(([name, deps]) => ({
      name,
      dependencies: [
        ...deps.map((d) => ({ name: d, kind: null })),
        ...(extra[name] ?? []),
      ],
    }));
}

describe("the crate-direction gate", () => {
  // each probe adds one edge (or removes one crate) so that exactly one rule can fire, and the message is compared whole
  test("passes the workspace as designed", () => {
    expect(findProblems(workspace()).problems).toEqual([]);
  });

  test("refuses an edge from one product into the other, and the host reaching the VM", () => {
    expect(
      findProblems(workspace({ neoc: [{ name: "vos-lua", kind: null }] }))
        .problems,
    ).toEqual([
      "neoc -> vos-lua: internal edge not allowed (allowed: neoc-vm)",
    ]);
    expect(
      findProblems(
        workspace({ "neoc-vm": [{ name: "vos-domain", kind: null }] }),
      ).problems,
    ).toEqual([
      "neoc-vm -> vos-domain: internal edge not allowed (allowed: neoc-agui and the VM crates)",
    ]);
    expect(
      findProblems(
        workspace({ "neoc-vm-core": [{ name: "vos-domain", kind: null }] }),
      ).problems,
    ).toEqual([
      "neoc-vm-core -> vos-domain: internal edge not allowed (allowed: neoc-agui and the VM crates)",
    ]);
    const host = findProblems(
      workspace({ "vos-host": [{ name: "neoc-vm", kind: null }] }),
    ).problems;
    expect(host).toHaveLength(1);
    expect(host[0]).toStartWith(
      "vos-host -> neoc-vm: internal edge not allowed",
    );
  });

  test("refuses an adapter that reaches another adapter", () => {
    const problems = findProblems(
      workspace({ "vos-http": [{ name: "vos-sqlite", kind: null }] }),
    ).problems;
    expect(problems).toHaveLength(1);
    expect(problems[0]).toStartWith(
      "vos-http -> vos-sqlite: internal edge not allowed",
    );
  });

  test("holds a framework crate to its home as a normal or a build dependency, not as a test dependency", () => {
    const fetchAxum = (kind: string | null) =>
      workspace({ "vos-fetch": [{ name: "axum", kind }] });
    expect(findProblems(fetchAxum(null)).problems).toEqual([
      "vos-fetch -> axum: framework crate is not allowed here as a normal dependency (allowed in: vos-http, vos-mcp, vos-host)",
    ]);
    expect(findProblems(fetchAxum("build")).problems).toEqual([
      "vos-fetch -> axum: framework crate is not allowed here as a build dependency (allowed in: vos-http, vos-mcp, vos-host)",
    ]);
    expect(findProblems(fetchAxum("dev")).problems).toEqual([]);
    expect(
      findProblems(workspace({ neoc: [{ name: "axum", kind: null }] }))
        .problems,
    ).toHaveLength(1);
    expect(
      findProblems(
        workspace({ "vos-lua": [{ name: "tonic", kind: null }] }),
      ).problems,
    ).toEqual([
      "vos-lua -> tonic: framework crate is not allowed here as a normal dependency (allowed in: none)",
    ]);
  });

  test("refuses a crate in the pure core that is not on its allow-list", () => {
    // not a framework crate, so only the allow-list rule can fire
    expect(
      findProblems(
        workspace({ "vos-domain": [{ name: "regex", kind: null }] }),
      ).problems,
    ).toEqual([
      "vos-domain -> regex: external crate is not on the core allow-list",
    ]);
  });

  test("refuses a framework crate in the pure core, with both rules that apply to it", () => {
    // tokio is on neither the allow-list nor the frameworks' homes' table, so the allow-list rule and the
    // framework-in-core rule both fire: each message must be there
    const problems = findProblems(
      workspace({ "vos-application": [{ name: "tokio", kind: null }] }),
    ).problems;
    expect(problems).toContain(
      "vos-application -> tokio: external crate is not on the core allow-list",
    );
    expect(problems).toContain(
      "vos-application -> tokio: framework or runtime crate in the core",
    );
    expect(problems).toHaveLength(2);
  });

  test("refuses a crate nobody placed, and a crate the table lists that the workspace lacks", () => {
    const stray = [...workspace(), { name: "mystery", dependencies: [] }];
    expect(findProblems(stray).problems).toEqual([
      "mystery: workspace crate is not placed (add it to INTERNAL in scripts/check-deps.ts)",
    ]);
    expect(findProblems(workspace({}, ["vos-codegen"])).problems).toEqual([
      "vos-codegen: listed in INTERNAL but not in the workspace",
    ]);
  });
});
