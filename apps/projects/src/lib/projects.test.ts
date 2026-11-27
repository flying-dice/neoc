import { describe, expect, test } from 'bun:test'
import { WEBSITE, createFailingBackend, createFakeBackend, issue, page, type FakeBackend } from './fakeApi'
import { Projects } from './projects.svelte'

const APP = { id: 2, key: 'APP', name: 'Mobile App', color: 'purple' } as const

const busy = (): FakeBackend =>
  createFakeBackend({
    projects: [WEBSITE, APP],
    issues: [
      issue(1, 'Write the home page'),
      issue(2, 'Fix the footer', { type: 'bug', priority: 'high' }),
      issue(3, 'Pick the colours', { status: 'doing' }),
      issue(4, 'Register the domain', { status: 'done' }),
      issue(5, 'Sketch the tab bar', { project: 2, key: 'APP-1' })
    ],
    pages: [page(1, 'Design'), page(2, 'Colours', { parent: 1 }), page(3, 'Launch plan'), page(4, 'Roadmap', { project: 2 })],
    bodies: { 1: 'How it should look.', 2: 'Blue, mostly.' }
  })

/** A window on a fake backend, loaded and listening. */
async function opened(backend: FakeBackend = createFakeBackend()): Promise<Projects> {
  const projects = new Projects(backend.rpc, backend.events)
  projects.listen()
  await projects.load()
  return projects
}

/** Wait until `done()` holds (an event is answered by a read of the workspace). */
async function until(done: () => boolean): Promise<void> {
  for (let i = 0; i < 200 && !done(); i++) await Bun.sleep(2)
  expect(done()).toBe(true)
}

const titles = (items: readonly { title: string }[]): string[] => items.map((i) => i.title)
const pageTitles = (projects: Projects): string[] => projects.pages.map((l) => l.page.title)
const find = <T extends { title: string }>(items: readonly T[], title: string): T => {
  const found = items.find((i) => i.title === title)
  if (!found) throw new Error(`nothing called ${title}`)
  return found
}
const pageCalled = (projects: Projects, title: string) => find(projects.pages.map((l) => l.page), title)

describe('a window that has just opened', () => {
  test('has no project to show on a new workspace', async () => {
    const projects = await opened()
    expect(projects.loaded).toBe(true)
    expect(projects.projects).toEqual([])
    expect(projects.project).toBeUndefined()
    expect(projects.failure).toBeNull()
    expect(projects.browsing).toBe(true)
  })

  test('shows the board of the first project', async () => {
    const projects = await opened(busy())
    expect(projects.project).toEqual(WEBSITE)
    expect(projects.tab).toBe('board')
    expect(titles(projects.columns.todo)).toEqual(['Fix the footer', 'Write the home page'])
    expect(titles(projects.columns.doing)).toEqual(['Pick the colours'])
    expect(titles(projects.columns.done)).toEqual(['Register the domain'])
    expect(projects.open.get(1)).toBe(3)
    expect(projects.open.get(2)).toBe(1)
  })

  test('keeps the reason when the backend cannot be reached', async () => {
    const projects = await opened(createFailingBackend('The host is not answering'))
    expect(projects.failure).toBe('The host is not answering')
    expect(projects.loaded).toBe(false)
  })
})

describe('a new project', () => {
  test('takes its key from its name until a key is typed', async () => {
    const projects = await opened()
    projects.begin()
    projects.draft({ name: 'Web Site' })
    expect(projects.form).toEqual({ name: 'Web Site', key: 'WS' })
    projects.draft({ key: 'site' })
    projects.draft({ name: 'Web Site 2' })
    expect(projects.form).toEqual({ name: 'Web Site 2', key: 'SITE' })
    // the key is emptied: it follows the name again
    projects.draft({ key: '' })
    expect(projects.form?.key).toBe('WS2')
  })

  test('cannot be sent without a name and a key that will do', async () => {
    const projects = await opened()
    projects.begin()
    expect(projects.ready).toBe(false)
    projects.draft({ name: 'Website', key: '1' })
    expect(projects.problems.key).toBe('A key is two to five letters or digits, a letter first, like WEB.')
    expect(projects.ready).toBe(false)
    await projects.addProject()
    expect(projects.projects).toEqual([])
    projects.draft({ key: 'web' })
    expect(projects.ready).toBe(true)
  })

  test('is added, shown, and the form is gone', async () => {
    const projects = await opened()
    projects.begin()
    projects.draft({ name: ' Website ' })
    await projects.addProject()
    expect(projects.form).toBeNull()
    expect(projects.project).toEqual(WEBSITE)
    expect(projects.browsing).toBe(false)
  })

  test('whose key is taken is refused at the form, not the window, until it is typed in again', async () => {
    const projects = await opened(busy())
    projects.begin()
    projects.draft({ name: 'Web Shop', key: 'web' })
    await projects.addProject()
    expect(projects.refused).toBe('Another project has the key WEB.')
    expect(projects.failure).toBeNull()
    expect(projects.form).toEqual({ name: 'Web Shop', key: 'WEB' })
    projects.draft({ key: 'shop' })
    expect(projects.refused).toBeNull()
  })

  test('given up leaves the project that was shown', async () => {
    const projects = await opened(busy())
    projects.begin()
    projects.cancel()
    expect(projects.form).toBeNull()
    expect(projects.project).toEqual(WEBSITE)
  })
})

describe('a project', () => {
  test('is deleted with what is in it; the window goes on to another', async () => {
    const projects = await opened(busy())
    await projects.removeProject(WEBSITE)
    expect(projects.projects).toEqual([APP])
    expect(projects.project).toEqual(APP)
    expect(titles(projects.issues)).toEqual(['Sketch the tab bar'])
    expect(projects.browsing).toBe(true)
  })

  test('picked closes the issue and the page that were open', async () => {
    const projects = await opened(busy())
    projects.openIssue(find(projects.issues, 'Fix the footer'))
    projects.select(APP)
    expect(projects.issue).toBeUndefined()
    expect(pageTitles(projects)).toEqual(['Roadmap'])
  })
})

describe('issues', () => {
  test('one typed is added to the project that is shown, under its next number', async () => {
    const projects = await opened(busy())
    projects.issueDraft = '  Add a contact form  '
    await projects.addIssue()
    expect(projects.issueDraft).toBe('')
    const added = find(projects.issues, 'Add a contact form')
    expect(added).toMatchObject({ key: 'WEB-5', type: 'task', priority: 'medium', status: 'todo' })
  })

  test('what was typed comes back when it could not be added', async () => {
    const backend = busy()
    const projects = await opened(backend)
    backend.rpc.addIssue = () => Promise.reject(new Error('The host is not answering'))
    projects.issueDraft = 'Add a contact form'
    await projects.addIssue()
    expect(projects.issueDraft).toBe('Add a contact form')
    expect(projects.failure).toBe('The host is not answering')
  })

  test('one moved changes column', async () => {
    const projects = await opened(busy())
    await projects.move(find(projects.issues, 'Fix the footer'), 'doing')
    expect(titles(projects.columns.todo)).toEqual(['Write the home page'])
    expect(titles(projects.columns.doing)).toEqual(['Fix the footer', 'Pick the colours'])
  })

  test('only what differs is sent, and a blank title never', async () => {
    const backend = busy()
    const projects = await opened(backend)
    const sent: unknown[] = []
    const update = backend.rpc.updateIssue
    backend.rpc.updateIssue = (params) => {
      sent.push(params)
      return update(params)
    }
    const footer = find(projects.issues, 'Fix the footer')
    await projects.changeIssue(footer, { title: '  ', type: 'bug', priority: 'high' })
    expect(sent).toEqual([])
    await projects.changeIssue(footer, { title: ' Fix the footer links ', description: 'They go nowhere.', type: 'bug' })
    expect(sent).toEqual([{ issue: 2, title: 'Fix the footer links', description: 'They go nowhere.' }])
  })

  test('one deleted while open is closed', async () => {
    const projects = await opened(busy())
    const footer = find(projects.issues, 'Fix the footer')
    projects.openIssue(footer)
    expect(projects.issue?.key).toBe('WEB-2')
    await projects.removeIssue(footer)
    expect(projects.issue).toBeUndefined()
    expect(titles(projects.issues)).not.toContain('Fix the footer')
  })
})

describe('pages', () => {
  test('are listed each followed by the pages under it', async () => {
    const projects = await opened(busy())
    expect(pageTitles(projects)).toEqual(['Design', 'Colours', 'Launch plan'])
  })

  test('one opened is read', async () => {
    const projects = await opened(busy())
    const reading = projects.openPage(pageCalled(projects, 'Design'))
    expect(projects.body).toBeNull()
    await reading
    expect(projects.page?.page.title).toBe('Design')
    expect(projects.body).toBe('How it should look.')
    expect(titles(projects.subpages)).toEqual(['Colours'])
  })

  test('one named is added, and opens ready to be written', async () => {
    const projects = await opened(busy())
    projects.pageDraft = 'Glossary'
    await projects.addPage()
    expect(projects.pageDraft).toBe('')
    expect(projects.page?.page).toMatchObject({ title: 'Glossary', parent: 0 })
    expect(projects.edit).toEqual({ title: 'Glossary', body: '' })
  })

  test('one named while a page is open goes under it', async () => {
    const projects = await opened(busy())
    await projects.openPage(pageCalled(projects, 'Design'))
    projects.pageDraft = 'Type'
    await projects.addPage()
    expect(projects.page).toMatchObject({ depth: 1, path: ['Design'] })
  })

  test('one rewritten keeps what was typed on Done, and drops it on Cancel', async () => {
    const backend = busy()
    const projects = await opened(backend)
    await projects.openPage(pageCalled(projects, 'Design'))
    projects.rewrite()
    projects.type({ body: 'Thrown away.' })
    projects.discard()
    expect(projects.edit).toBeNull()
    expect(projects.body).toBe('How it should look.')
    projects.rewrite()
    projects.type({ title: 'Design notes', body: 'Calm and plain.' })
    await projects.save()
    expect(projects.edit).toBeNull()
    expect(projects.page?.page.title).toBe('Design notes')
    expect(projects.body).toBe('Calm and plain.')
    expect((await backend.rpc.readPage({ page: 1 })).body).toBe('Calm and plain.')
  })

  test('one without a title is not saved', async () => {
    const projects = await opened(busy())
    await projects.openPage(pageCalled(projects, 'Design'))
    projects.rewrite()
    projects.type({ title: '  ' })
    await projects.save()
    expect(projects.edit).toEqual({ title: '  ', body: 'How it should look.' })
  })

  test('one left goes back to the page it is under, then to the list', async () => {
    const projects = await opened(busy())
    await projects.openPage(pageCalled(projects, 'Colours'))
    await projects.closePage()
    expect(projects.page?.page.title).toBe('Design')
    expect(projects.body).toBe('How it should look.')
    await projects.closePage()
    expect(projects.page).toBeUndefined()
  })

  test('one deleted takes the pages under it, and the window goes on to the list', async () => {
    const projects = await opened(busy())
    const design = pageCalled(projects, 'Design')
    await projects.openPage(design)
    await projects.removePage(design)
    expect(projects.page).toBeUndefined()
    expect(pageTitles(projects)).toEqual(['Launch plan'])
  })

  test('a page under another, deleted, leaves the window on the one it was under', async () => {
    const projects = await opened(busy())
    const colours = pageCalled(projects, 'Colours')
    await projects.openPage(colours)
    await projects.removePage(colours)
    expect(projects.page?.page.title).toBe('Design')
  })
})

describe('what happens behind the window', () => {
  test('an agent raises an issue: the window reads the workspace again', async () => {
    const backend = busy()
    const projects = await opened(backend)
    await backend.rpc.addIssue({ project: 1, title: 'From the agent' })
    backend.emit('projects_changed')
    await until(() => titles(projects.issues).includes('From the agent'))
  })

  test('an agent rewrites the open page: its text is read again, unless it is being rewritten here', async () => {
    const backend = busy()
    const projects = await opened(backend)
    await projects.openPage(pageCalled(projects, 'Design'))
    backend.rewrite(1, 'Said the agent.')
    backend.emit('projects_changed')
    await until(() => projects.body === 'Said the agent.')
    projects.rewrite()
    projects.type({ body: 'Mine.' })
    backend.rewrite(1, 'Said the agent, again.')
    backend.emit('projects_changed')
    await Bun.sleep(20)
    expect(projects.edit?.body).toBe('Mine.')
  })

  test('the project that is shown is deleted elsewhere: the window goes on to another', async () => {
    const backend = busy()
    const projects = await opened(backend)
    await backend.rpc.removeProject({ project: 1 })
    backend.emit('host:reconnected')
    await until(() => projects.project?.id === 2)
  })
})
