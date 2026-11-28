import { describe, expect, test } from 'bun:test'
import { SKILLS_DIR, WORKDIR, blankFile, blankServer, blankVariable, copilotDraftOf, copilotParamsOf, skillDraftOf, skillParamsOf } from './form'

type Copilot = VosApp.Copilot.Copilot

const CODER: Copilot = {
  id: 'c1',
  name: 'Coder',
  dockerfile: 'FROM oven/bun\n',
  command: ['npx', '-y', '@zed-industries/claude-code-acp'],
  workdir: '/work',
  skills_dir: '/root/.claude/skills',
  env: [
    { name: 'ANTHROPIC_API_KEY', secret: { service: 'anthropic', account: 'ada' } },
    { name: 'LANG', value: 'en_GB.UTF-8' }
  ],
  mcp: [
    { type: 'stdio', name: 'files', command: 'mcp-files', args: ['--root', '/work dir'], env: [{ name: 'DEBUG', value: '1' }] },
    { type: 'http', name: 'tickets', url: 'http://host.docker.internal:7410/mcp', headers: [{ name: 'Authorization', secret: { service: 'tickets', account: 'ada' } }] }
  ],
  skills: ['s1'],
  image: '',
  current: false,
  building: false,
  problem: ''
}

describe('the copilot form', () => {
  test('starts a new copilot with the usual folders and nothing else', () => {
    const draft = copilotDraftOf(null)
    expect(draft).toMatchObject({ copilot: null, name: '', dockerfile: '', command: '', workdir: WORKDIR, skillsDir: SKILLS_DIR, env: [], mcp: [], skills: [] })
  })

  test('sends back what a copilot has, as it has it', () => {
    const draft = copilotDraftOf(CODER)
    expect(draft.command).toBe('npx -y @zed-industries/claude-code-acp')
    expect(draft.mcp[0]!.args).toBe('--root "/work dir"')
    expect(copilotParamsOf(draft)).toEqual({
      name: 'Coder',
      dockerfile: 'FROM oven/bun\n',
      command: CODER.command,
      workdir: '/work',
      skills_dir: '/root/.claude/skills',
      env: CODER.env,
      mcp: CODER.mcp,
      skills: ['s1']
    })
  })

  test('sends a variable with a keychain item without its value, and one written out without an item', () => {
    const draft = copilotDraftOf(null)
    draft.env = [
      { ...blankVariable(), name: ' TOKEN ', value: 'left over', secret: { service: 'gh', account: 'ada' } },
      { ...blankVariable(), name: 'EMPTY' }
    ]
    expect(copilotParamsOf(draft).env).toEqual([
      { name: 'TOKEN', secret: { service: 'gh', account: 'ada' } },
      { name: 'EMPTY', value: '' }
    ])
  })

  test('leaves out a row nothing was typed in', () => {
    const draft = copilotDraftOf(null)
    draft.env = [blankVariable(), { ...blankVariable(), name: 'A', value: '1' }]
    expect(copilotParamsOf(draft).env).toEqual([{ name: 'A', value: '1' }])
  })

  test("sends what a server's type has, and not what the other type would have", () => {
    const draft = copilotDraftOf(null)
    const server = { ...blankServer('stdio'), name: 'web', command: 'mcp-web', args: '-v', url: 'http://left.over' }
    draft.mcp = [server]
    expect(copilotParamsOf(draft).mcp).toEqual([{ type: 'stdio', name: 'web', command: 'mcp-web', args: ['-v'], env: [] }])
    draft.mcp = [{ ...server, type: 'http', url: 'https://example.com/mcp' }]
    expect(copilotParamsOf(draft).mcp).toEqual([{ type: 'http', name: 'web', url: 'https://example.com/mcp', headers: [] }])
  })

  test('changing the form changes nothing of the copilot it was made from', () => {
    const draft = copilotDraftOf(CODER)
    draft.env[0]!.secret!.service = 'other'
    draft.skills.push('s2')
    expect(CODER.env[0]!.secret!.service).toBe('anthropic')
    expect(CODER.skills).toEqual(['s1'])
  })
})

describe('the skill form', () => {
  test('sends a skill as it is, and leaves out a file nothing was typed in', () => {
    const draft = skillDraftOf({ id: 's1', name: 'review', text: '# Review', files: [{ path: 'check.sh', text: 'echo ok' }] })
    draft.files.push(blankFile())
    expect(skillParamsOf(draft)).toEqual({ name: 'review', text: '# Review', files: [{ path: 'check.sh', text: 'echo ok' }] })
  })
})
