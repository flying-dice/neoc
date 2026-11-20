import { describe, expect, test } from 'bun:test'
import { findProblems, type WorkspaceCrate } from './check-deps'

/** The workspace as it should be: every crate in the table, each with only the edges it may have. */
function workspace(extra: Record<string, { name: string; kind: string | null }[]> = {}): WorkspaceCrate[] {
  const edges: Record<string, string[]> = {
    'agentos-domain': [],
    'agentos-application': ['agentos-domain'],
    'agentos-lua': ['agentos-domain', 'agentos-application', 'neoc-vm'],
    'neoc-vm': ['mlua', 'rusqlite'],
    neoc: ['neoc-vm', 'mlua'],
    'agentos-sqlite': ['agentos-domain', 'agentos-application', 'rusqlite'],
    'agentos-fetch': ['agentos-application'],
    'agentos-mcp': ['agentos-domain', 'agentos-application', 'rmcp', 'axum'],
    'agentos-acp': ['agentos-domain', 'agentos-application', 'agent-client-protocol'],
    'agentos-agui': ['agentos-domain', 'agentos-application', 'axum'],
    'agentos-http': ['agentos-domain', 'agentos-application', 'axum'],
    'agentos-codegen': ['agentos-domain'],
    'agentos-host': [
      'agentos-domain',
      'agentos-application',
      'agentos-lua',
      'agentos-sqlite',
      'agentos-fetch',
      'agentos-mcp',
      'agentos-acp',
      'agentos-agui',
      'agentos-http',
      'axum',
      'axum-server'
    ]
  }
  return Object.entries(edges).map(([name, deps]) => ({
    name,
    dependencies: [...deps.map((d) => ({ name: d, kind: null })), ...(extra[name] ?? [])]
  }))
}

describe('the crate-direction gate', () => {
  test('passes the workspace as designed', () => {
    expect(findProblems(workspace()).problems).toEqual([])
  })

  test('refuses an edge from one product into the other, and the host reaching the VM', () => {
    const into = findProblems(workspace({ neoc: [{ name: 'agentos-lua', kind: null }] })).problems
    expect(into.join()).toContain('neoc -> agentos-lua: internal edge not allowed')
    const vm = findProblems(workspace({ 'neoc-vm': [{ name: 'agentos-domain', kind: null }] })).problems
    expect(vm.join()).toContain('neoc-vm -> agentos-domain')
    const host = findProblems(workspace({ 'agentos-host': [{ name: 'neoc-vm', kind: null }] })).problems
    expect(host.join()).toContain('agentos-host -> neoc-vm')
  })

  test('refuses an adapter that reaches another adapter', () => {
    const problems = findProblems(workspace({ 'agentos-http': [{ name: 'agentos-sqlite', kind: null }] })).problems
    expect(problems.join()).toContain('agentos-http -> agentos-sqlite')
  })

  test('holds a framework crate to its home as a normal or a build dependency, not as a test dependency', () => {
    const normal = findProblems(workspace({ 'agentos-fetch': [{ name: 'axum', kind: null }] })).problems
    expect(normal.join()).toContain('agentos-fetch -> axum: framework crate is not allowed here as a normal dependency')
    const build = findProblems(workspace({ 'agentos-fetch': [{ name: 'axum', kind: 'build' }] })).problems
    expect(build.join()).toContain('as a build dependency')
    expect(findProblems(workspace({ 'agentos-fetch': [{ name: 'axum', kind: 'dev' }] })).problems).toEqual([])
    expect(findProblems(workspace({ neoc: [{ name: 'axum', kind: null }] })).problems.join()).toContain('neoc -> axum')
    expect(findProblems(workspace({ 'agentos-lua': [{ name: 'tonic', kind: null }] })).problems.join()).toContain(
      'agentos-lua -> tonic'
    )
  })

  test('refuses a framework crate in the pure core and a crate nobody placed', () => {
    const core = findProblems(workspace({ 'agentos-domain': [{ name: 'tokio', kind: null }] })).problems.join()
    expect(core).toContain('agentos-domain -> tokio')
    const stray = [...workspace(), { name: 'mystery', dependencies: [] }]
    expect(findProblems(stray).problems.join()).toContain('mystery: workspace crate is not placed')
  })
})
