import { describe, expect, test } from "bun:test";
import { findProblems, type WorkspaceCrate } from "./check-deps";

/** The workspace as it should be: every crate in the table, each with only the edges it may have. */
function workspace(
  extra: Record<string, { name: string; kind: string | null }[]> = {},
  without: string[] = [],
): WorkspaceCrate[] {
  const edges: Record<string, string[]> = {
    "agentos-domain": [],
    "agentos-application": ["agentos-domain"],
    "agentos-lua": ["agentos-domain", "agentos-application", "neoc-vm"],
    "neoc-vm": ["mlua", "rusqlite"],
    neoc: ["neoc-vm", "mlua"],
    "agentos-sqlite": ["agentos-domain", "agentos-application", "rusqlite"],
    "agentos-fetch": ["agentos-application"],
    "agentos-mcp": ["agentos-domain", "agentos-application", "rmcp", "axum"],
    "agentos-acp": [
      "agentos-domain",
      "agentos-application",
      "agent-client-protocol",
    ],
    "agentos-agui": ["agentos-domain", "agentos-application", "axum"],
    "agentos-http": ["agentos-domain", "agentos-application", "axum"],
    "agentos-codegen": ["agentos-domain"],
    "agentos-host": [
      "agentos-domain",
      "agentos-application",
      "agentos-lua",
      "agentos-sqlite",
      "agentos-fetch",
      "agentos-mcp",
      "agentos-acp",
      "agentos-agui",
      "agentos-http",
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
      findProblems(workspace({ neoc: [{ name: "agentos-lua", kind: null }] }))
        .problems,
    ).toEqual([
      "neoc -> agentos-lua: internal edge not allowed (allowed: neoc-vm)",
    ]);
    expect(
      findProblems(
        workspace({ "neoc-vm": [{ name: "agentos-domain", kind: null }] }),
      ).problems,
    ).toEqual([
      "neoc-vm -> agentos-domain: internal edge not allowed (allowed: none)",
    ]);
    const host = findProblems(
      workspace({ "agentos-host": [{ name: "neoc-vm", kind: null }] }),
    ).problems;
    expect(host).toHaveLength(1);
    expect(host[0]).toStartWith(
      "agentos-host -> neoc-vm: internal edge not allowed",
    );
  });

  test("refuses an adapter that reaches another adapter", () => {
    const problems = findProblems(
      workspace({ "agentos-http": [{ name: "agentos-sqlite", kind: null }] }),
    ).problems;
    expect(problems).toHaveLength(1);
    expect(problems[0]).toStartWith(
      "agentos-http -> agentos-sqlite: internal edge not allowed",
    );
  });

  test("holds a framework crate to its home as a normal or a build dependency, not as a test dependency", () => {
    const fetchAxum = (kind: string | null) =>
      workspace({ "agentos-fetch": [{ name: "axum", kind }] });
    expect(findProblems(fetchAxum(null)).problems).toEqual([
      "agentos-fetch -> axum: framework crate is not allowed here as a normal dependency (allowed in: agentos-http, agentos-mcp, agentos-agui, agentos-host)",
    ]);
    expect(findProblems(fetchAxum("build")).problems).toEqual([
      "agentos-fetch -> axum: framework crate is not allowed here as a build dependency (allowed in: agentos-http, agentos-mcp, agentos-agui, agentos-host)",
    ]);
    expect(findProblems(fetchAxum("dev")).problems).toEqual([]);
    expect(
      findProblems(workspace({ neoc: [{ name: "axum", kind: null }] }))
        .problems,
    ).toHaveLength(1);
    expect(
      findProblems(
        workspace({ "agentos-lua": [{ name: "tonic", kind: null }] }),
      ).problems,
    ).toEqual([
      "agentos-lua -> tonic: framework crate is not allowed here as a normal dependency (allowed in: none)",
    ]);
  });

  test("refuses a crate in the pure core that is not on its allow-list", () => {
    // not a framework crate, so only the allow-list rule can fire
    expect(
      findProblems(
        workspace({ "agentos-domain": [{ name: "regex", kind: null }] }),
      ).problems,
    ).toEqual([
      "agentos-domain -> regex: external crate is not on the core allow-list",
    ]);
  });

  test("refuses a framework crate in the pure core, with both rules that apply to it", () => {
    // tokio is on neither the allow-list nor the frameworks' homes' table, so the allow-list rule and the
    // framework-in-core rule both fire: each message must be there
    const problems = findProblems(
      workspace({ "agentos-application": [{ name: "tokio", kind: null }] }),
    ).problems;
    expect(problems).toContain(
      "agentos-application -> tokio: external crate is not on the core allow-list",
    );
    expect(problems).toContain(
      "agentos-application -> tokio: framework or runtime crate in the core",
    );
    expect(problems).toHaveLength(2);
  });

  test("refuses a crate nobody placed, and a crate the table lists that the workspace lacks", () => {
    const stray = [...workspace(), { name: "mystery", dependencies: [] }];
    expect(findProblems(stray).problems).toEqual([
      "mystery: workspace crate is not placed (add it to INTERNAL in scripts/check-deps.ts)",
    ]);
    expect(findProblems(workspace({}, ["agentos-codegen"])).problems).toEqual([
      "agentos-codegen: listed in INTERNAL but not in the workspace",
    ]);
  });
});
