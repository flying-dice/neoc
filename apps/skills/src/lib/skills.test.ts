import { describe, expect, test } from 'bun:test'
import { DEPLOY, REVIEW, createFailingBackend, createFakeBackend, type FakeBackend } from './fakeApi'
import { Skills } from './skills.svelte'

/** A window on a fake backend, loaded and listening. */
async function opened(backend: FakeBackend = createFakeBackend()): Promise<Skills> {
  const skills = new Skills(backend.rpc, backend.events)
  skills.listen()
  await skills.load()
  return skills
}

/** Wait until `done()` holds (an event is answered by a call, on a later turn of the event loop). */
async function until(done: () => boolean): Promise<void> {
  for (let i = 0; i < 200 && !done(); i++) await Bun.sleep(2)
  expect(done()).toBe(true)
}

const names = (skills: Skills): string[] => skills.skills.map((s) => s.name)

describe('a window that has just opened', () => {
  test('shows nothing until the first answer, then the skills by name', async () => {
    const backend = createFakeBackend()
    const skills = new Skills(backend.rpc, backend.events)
    expect(skills.loaded).toBe(false)
    await skills.load()
    expect(skills.loaded).toBe(true)
    expect(names(skills)).toEqual(['deploy', 'review'])
    expect(skills.failure).toBeNull()
  })

  test('keeps the reason when the backend cannot be reached', async () => {
    const skills = await opened(createFailingBackend('The host is not answering.'))
    expect(skills.loaded).toBe(false)
    expect(skills.failure).toBe('The host is not answering.')
  })

  test('reads the skills again when the host connection comes back', async () => {
    const backend = createFakeBackend()
    const skills = await opened(backend)
    backend.elsewhere('notes', '# Notes')
    expect(names(skills)).toEqual(['deploy', 'review'])
    backend.emit('host:reconnected')
    await until(() => names(skills).includes('notes'))
  })
})

describe('the skill form', () => {
  test('adds a skill with a file, and closes', async () => {
    const skills = await opened()
    skills.edit(null)
    expect(skills.savable).toBe(false)
    Object.assign(skills.form!, { name: ' changelog ', text: '---\ndescription: Write the changelog.\n---\n# Changelog' })
    skills.form!.files.push({ key: 99, path: ' scripts/log.sh ', text: 'git log' })
    expect(skills.savable).toBe(true)
    expect(await skills.save()).toBe(true)
    expect(skills.form).toBeNull()
    expect(names(skills)).toEqual(['changelog', 'deploy', 'review'])
    const added = skills.skills[0]!
    expect(added.description).toBe('Write the changelog.')
    expect(added.files).toEqual([{ path: 'scripts/log.sh', text: 'git log' }])
  })

  test('stays open with the reason when the backend refuses it', async () => {
    const skills = await opened()
    skills.edit(REVIEW)
    skills.form!.name = 'deploy'
    expect(await skills.save()).toBe(false)
    expect(skills.form?.name).toBe('deploy')
    expect(skills.failure).toBe('There is a skill named deploy already.')
    skills.form!.name = 'my review'
    await skills.save()
    expect(skills.failure).toContain("A skill's name is the name of its folder")
    skills.form!.name = 'review'
    skills.form!.files[0]!.path = '../escape.sh'
    await skills.save()
    expect(skills.failure).toBe("../escape.sh must be a path in the skill's folder, with no empty part, no . and no ..")
    skills.close()
    expect(skills.failure).toBeNull()
    expect(skills.form).toBeNull()
  })

  test('changes a skill: its name, its text and its files', async () => {
    const skills = await opened()
    skills.edit(REVIEW)
    expect(skills.form!.files.map((f) => f.path)).toEqual(['scripts/check.sh'])
    skills.form!.name = 'code-review'
    skills.form!.files = []
    expect(await skills.save()).toBe(true)
    expect(names(skills)).toEqual(['code-review', 'deploy'])
    expect(skills.skills[0]!.files).toEqual([])
  })

  test('is not saved without a name or a SKILL.md', async () => {
    const skills = await opened()
    skills.edit(null)
    skills.form!.name = 'empty'
    skills.form!.text = '   '
    expect(skills.savable).toBe(false)
    expect(await skills.save()).toBe(false)
    expect(names(skills)).toEqual(['deploy', 'review'])
  })

  test('removes the skill it changes, once the person says so', async () => {
    const skills = await opened()
    skills.edit(DEPLOY)
    skills.asking = true
    expect(await skills.remove()).toBe(true)
    expect(skills.asking).toBe(false)
    expect(skills.form).toBeNull()
    expect(names(skills)).toEqual(['review'])
  })

  test('a new skill has nothing to remove', async () => {
    const skills = await opened()
    skills.edit(null)
    expect(await skills.remove()).toBe(false)
    expect(names(skills)).toEqual(['deploy', 'review'])
  })
})
