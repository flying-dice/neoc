import { describe, expect, test } from 'bun:test'
import { CODER, ECHO, PICTURE, PICTURED, SCRIPTED } from './fakeApi'
import { SKILLS_DIR, addParamsOf, agentDraftOf, blankServer, blankVariable, savable, updateParamsOf, type AgentDraft } from './form'

type Agent = VosApp.Agents.Agent

const editing = (draft: AgentDraft): AgentDraft & { agent: Agent } => draft as AgentDraft & { agent: Agent }

describe('a new agent', () => {
  test('starts with the folder its type has when none is given, and nothing else', () => {
    expect(agentDraftOf(null, 'remote')).toMatchObject({ agent: null, kind: 'remote', name: '', url: '', folder: '/', command: '', env: [], mcp: [] })
    expect(agentDraftOf(null, 'container')).toMatchObject({ kind: 'container', folder: '/workspace', image: '', skillsDir: SKILLS_DIR, skills: [] })
    // a program's folder is one of this machine's, and has to be chosen
    expect(agentDraftOf(null, 'program')).toMatchObject({ kind: 'program', folder: '' })
    expect(agentDraftOf(null).kind).toBe('remote')
  })

  test('can be saved once it has a name and what its type cannot do without', () => {
    const remote = { ...agentDraftOf(null, 'remote'), name: 'Ada' }
    expect(savable(remote)).toBe(false)
    expect(savable({ ...remote, url: 'ws://example.com/acp' })).toBe(true)

    const program = { ...agentDraftOf(null, 'program'), name: 'Echo', command: 'python3 echo.py' }
    expect(savable(program)).toBe(false)
    expect(savable({ ...program, folder: '/home/ada' })).toBe(true)
    expect(savable({ ...program, folder: '/home/ada', command: '  ' })).toBe(false)

    const container = { ...agentDraftOf(null, 'container'), name: 'Coder', command: 'claude-code-acp' }
    expect(savable(container)).toBe(false)
    expect(savable({ ...container, image: 'agent-coder:1' })).toBe(true)
    expect(savable({ ...container, image: 'agent-coder:1', name: ' ' })).toBe(false)
  })

  test('is sent with its type and only what its type takes', () => {
    const remote = { ...agentDraftOf(null, 'remote'), name: 'Ada', url: 'ws://example.com/acp', token: 'secret', command: 'left over', image: 'left:over' }
    expect(addParamsOf(remote)).toEqual({ kind: 'remote', name: 'Ada', url: 'ws://example.com/acp', folder: '/', mcp: [], token: 'secret' })

    const program = { ...agentDraftOf(null, 'program'), name: 'Echo', url: 'ws://left.over', token: 'left over', command: 'python3 -u "echo agent.py"', folder: '/home/ada' }
    expect(addParamsOf(program)).toEqual({ kind: 'program', name: 'Echo', folder: '/home/ada', command: ['python3', '-u', 'echo agent.py'], env: [], mcp: [] })

    const container = { ...agentDraftOf(null, 'container'), name: 'Coder', image: ' agent-coder:1 ', command: 'claude-code-acp', skills: ['review'] }
    expect(addParamsOf(container)).toEqual({
      kind: 'container',
      name: 'Coder',
      folder: '/workspace',
      command: ['claude-code-acp'],
      env: [],
      image: 'agent-coder:1',
      skills_dir: SKILLS_DIR,
      skills: ['review'],
      mcp: []
    })
  })

  test('is sent with its picture only when it has one', () => {
    const draft = { ...agentDraftOf(null, 'remote'), name: 'Ada', url: 'ws://example.com/acp' }
    expect(addParamsOf(draft)).not.toHaveProperty('avatar')
    expect(addParamsOf({ ...draft, avatar: PICTURE })).toMatchObject({ avatar: PICTURE })
  })
})

describe('an agent that is there', () => {
  test('sends back what a container agent has, as it has it', () => {
    const draft = agentDraftOf(CODER)
    expect(draft.command).toBe('claude-code-acp')
    expect(draft.mcp[0]!.args).toBe('--root /workspace')
    expect(updateParamsOf(editing(draft))).toEqual({
      agent: CODER.id,
      name: CODER.name,
      folder: CODER.folder,
      command: CODER.command,
      env: CODER.env,
      image: CODER.image,
      skills_dir: CODER.skills_dir,
      skills: CODER.skills,
      mcp: CODER.mcp
    })
  })

  test('sends back what a program agent has', () => {
    expect(updateParamsOf(editing(agentDraftOf(ECHO)))).toEqual({ agent: ECHO.id, name: ECHO.name, folder: ECHO.folder, command: ECHO.command, env: ECHO.env, mcp: [] })
  })

  test("keeps its type, whatever the form is opened with", () => {
    expect(agentDraftOf(CODER, 'remote').kind).toBe('container')
  })

  test('a remote one sends whether to forget its token, and a new token only when one is typed', () => {
    const draft = agentDraftOf({ ...SCRIPTED, token: true })
    expect(updateParamsOf(editing(draft))).toEqual({ agent: SCRIPTED.id, name: SCRIPTED.name, url: SCRIPTED.url, folder: SCRIPTED.folder, mcp: [], forget: false })
    expect(updateParamsOf(editing({ ...draft, token: 'new' }))).toMatchObject({ token: 'new' })
  })

  test('sends its picture only when it changed: a new one, or that it is removed', () => {
    const draft = agentDraftOf(PICTURED)
    expect(updateParamsOf(editing(draft))).not.toHaveProperty('avatar')
    expect(updateParamsOf(editing(draft))).not.toHaveProperty('remove_avatar')
    expect(updateParamsOf(editing({ ...draft, avatar: '' }))).toMatchObject({ remove_avatar: true })
    const other = PICTURE.replace('png', 'webp')
    expect(updateParamsOf(editing({ ...draft, avatar: other }))).toMatchObject({ avatar: other })
  })

  test('changing the form changes nothing of the agent it was made from', () => {
    const draft = agentDraftOf(CODER)
    draft.env[0]!.secret!.service = 'other'
    draft.skills.push('triage')
    expect(CODER.env[0]!.secret!.service).toBe('anthropic')
    expect(CODER.skills).toEqual(['review'])
  })
})

describe('variables and MCP servers', () => {
  test('a variable with a keychain item is sent without its value, and one written out without an item', () => {
    const draft = { ...agentDraftOf(null, 'program'), name: 'Echo', command: 'echo', folder: '/' }
    draft.env = [
      { ...blankVariable(), name: ' TOKEN ', value: 'left over', secret: { service: 'gh', account: 'ada' } },
      { ...blankVariable(), name: 'EMPTY' }
    ]
    expect(addParamsOf(draft).env).toEqual([
      { name: 'TOKEN', secret: { service: 'gh', account: 'ada' } },
      { name: 'EMPTY', value: '' }
    ])
  })

  test('a row nothing was typed in is left out', () => {
    const draft = { ...agentDraftOf(null, 'program'), name: 'Echo', command: 'echo', folder: '/' }
    draft.env = [blankVariable(), { ...blankVariable(), name: 'A', value: '1' }]
    expect(addParamsOf(draft).env).toEqual([{ name: 'A', value: '1' }])
  })

  test("a server is sent with what its type has, and not what the other type would have", () => {
    const draft = { ...agentDraftOf(null, 'remote'), name: 'Ada', url: 'ws://example.com/acp' }
    const server = { ...blankServer('stdio'), name: 'web', command: 'mcp-web', args: '-v', url: 'http://left.over' }
    draft.mcp = [server]
    expect(addParamsOf(draft).mcp).toEqual([{ type: 'stdio', name: 'web', command: 'mcp-web', args: ['-v'], env: [] }])
    draft.mcp = [{ ...server, type: 'http', url: 'https://example.com/mcp' }]
    expect(addParamsOf(draft).mcp).toEqual([{ type: 'http', name: 'web', url: 'https://example.com/mcp', headers: [] }])
  })

  test('a remote agent keeps its MCP servers but sends no environment', () => {
    const draft = { ...agentDraftOf(null, 'remote'), name: 'Ada', url: 'ws://example.com/acp', mcp: [{ ...blankServer('http'), name: 'tickets', url: 'https://t.example.com/mcp' }] }
    draft.env = [{ ...blankVariable(), name: 'LEFT', value: 'over' }]
    const sent = addParamsOf(draft)
    expect(sent.mcp).toHaveLength(1)
    expect(sent).not.toHaveProperty('env')
  })
})
