import { describe, expect, test } from 'bun:test'
import { Containers } from './containers.svelte'
import { createFakeBackend, image, recipe, type FakeBackend, type FakeOptions } from './fakeApi'
import { statusOf } from './recipes.svelte'

/** A window on a fake backend, loaded and listening. */
async function opened(options: FakeOptions = {}): Promise<{ containers: Containers; backend: FakeBackend }> {
  const backend = createFakeBackend(options)
  const containers = new Containers(backend.rpc, backend.events)
  containers.listen()
  await containers.load()
  return { containers, backend }
}

/** Wait until `done()` holds (an event is answered by a call, on a later turn of the event loop). */
async function until(done: () => boolean): Promise<void> {
  for (let i = 0; i < 200 && !done(); i++) await Bun.sleep(2)
  expect(done()).toBe(true)
}

const DOCKERFILE = 'FROM alpine:3.20\nRUN apk add --no-cache curl'
const tagsOf = (containers: Containers): string[] => containers.resources.images.flatMap((i) => i.tags)

describe("a recipe's status", () => {
  const built = { ...image(['tools:1']), containers: [] }
  const tools = recipe('tools:1', { image: built.id, current: true })

  test('says it is built, by the start of the id of its image', () => {
    expect(statusOf(tools, null, [built])).toEqual({ tone: 'done', text: `Built: ${built.id.slice(7, 19)}` })
  })
  test('is under way while this window builds it, or the backend says so', () => {
    expect(statusOf(tools, { lines: [], status: 'running' }, [built]).tone).toBe('running')
    expect(statusOf({ ...tools, building: true }, null, [built])).toEqual({ tone: 'running', text: 'Building…' })
    expect(statusOf(null, { lines: [], status: 'running' }, []).tone).toBe('running')
  })
  test("says why the last build failed, in the backend's words", () => {
    expect(statusOf({ ...tools, problem: 'The image could not be built: no' }, null, [built])).toEqual({ tone: 'failed', text: 'Failed: The image could not be built: no' })
  })
  test('says one never built, or one not saved yet, is not built', () => {
    expect(statusOf(recipe('later:1'), null, []).text).toBe('Not built yet.')
    expect(statusOf(null, null, []).text).toBe('Not built yet.')
  })
  test('says when the recipe changed since, and when the image it built is gone', () => {
    expect(statusOf({ ...tools, current: false }, null, [built]).text).toBe(`Changed since the last build (${built.id.slice(7, 19)}).`)
    expect(statusOf(tools, null, []).text).toBe('The image it built is gone: build it again.')
  })
})

describe('the recipes', () => {
  test('are read with the window', async () => {
    const { containers } = await opened({ recipes: [recipe('tools:1'), recipe('alpha:2')] })
    expect(containers.recipes.all.map((r) => r.tag)).toEqual(['alpha:2', 'tools:1'])
  })

  test('one is added by its name and Dockerfile, a name without a tag taking latest, and the form closes', async () => {
    const { containers } = await opened()
    const recipes = containers.recipes
    recipes.compose()
    expect(recipes.savable).toBe(false)
    recipes.form!.draft = { tag: 'tools', dockerfile: DOCKERFILE }
    expect(recipes.savable).toBe(true)
    await recipes.save()
    expect(recipes.form).toBeNull()
    expect(recipes.all.map((r) => [r.tag, r.dockerfile])).toEqual([['tools:latest', DOCKERFILE]])
  })

  test("a name another recipe builds is refused, and the form stays with the backend's reason", async () => {
    const { containers } = await opened({ recipes: [recipe('tools:1')] })
    const recipes = containers.recipes
    recipes.compose()
    recipes.form!.draft = { tag: 'tools:1', dockerfile: DOCKERFILE }
    await recipes.save()
    expect(recipes.form?.failure).toBe('Another recipe builds tools:1 already.')
    expect(recipes.form?.saving).toBe(false)
    expect(recipes.all).toHaveLength(1)
  })

  test('Build saves a new recipe, stays open on it, and the log fills as the build goes; the image is then in the list', async () => {
    const { containers } = await opened()
    const recipes = containers.recipes
    recipes.compose()
    recipes.form!.draft = { tag: 'tools:1', dockerfile: DOCKERFILE }
    await recipes.build()
    const saved = recipes.editing
    expect(saved?.tag).toBe('tools:1')
    expect(recipes.form?.recipe).toBe(saved!.id)
    await until(() => recipes.buildOf(saved!.id)?.status === 'done')
    expect(recipes.buildOf(saved!.id)?.lines).toEqual([
      'Building tools:1',
      'Step 1/2 : FROM alpine:3.20',
      'Step 2/2 : RUN apk add --no-cache curl',
      'Successfully tagged tools:1',
      'Built tools:1.'
    ])
    await until(() => tagsOf(containers).includes('tools:1') && recipes.editing?.image !== '')
    expect(statusOf(recipes.editing, recipes.buildOf(saved!.id), containers.resources.images).tone).toBe('done')
  })

  test('is under way until the build ends, as the window and the recipe say', async () => {
    const { containers, backend } = await opened({ recipes: [recipe('tools:1')], holdBuilds: true })
    const recipes = containers.recipes
    recipes.edit(recipes.all[0]!)
    await recipes.build()
    expect(recipes.building(recipes.editing)).toBe(true)
    expect(statusOf(recipes.editing, recipes.buildOf(recipes.editing!.id), []).text).toBe('Building…')
    backend.finish()
    await until(() => !recipes.building(recipes.editing) && recipes.editing?.image !== '')
  })

  test('an edit built again moves the name to the new image; the one before keeps no name', async () => {
    const { containers } = await opened()
    const recipes = containers.recipes
    recipes.compose()
    recipes.form!.draft = { tag: 'tools:1', dockerfile: DOCKERFILE }
    await recipes.build()
    await until(() => recipes.editing?.image !== '' && tagsOf(containers).includes('tools:1'))
    const first = recipes.editing!.image

    recipes.form!.draft.dockerfile = `${DOCKERFILE} git`
    await recipes.save()
    expect(recipes.all[0]!.current).toBe(false)
    recipes.edit(recipes.all[0]!)
    await recipes.build()
    await until(() => recipes.editing?.image !== first && recipes.editing?.current === true)
    await until(() => containers.resources.images.find((i) => i.tags.includes('tools:1'))?.id === recipes.editing!.image)
    expect(containers.resources.images.find((i) => i.id === first)?.tags).toEqual([])
  })

  test("a Dockerfile that fails says why, in Docker's words, and keeps the recipe", async () => {
    const { containers } = await opened()
    const recipes = containers.recipes
    recipes.compose()
    recipes.form!.draft = { tag: 'broken:1', dockerfile: 'FROM alpine:3.20\nRUN exit 3' }
    await recipes.build()
    const id = recipes.editing!.id
    await until(() => recipes.buildOf(id)?.status === 'failed' && recipes.editing?.problem !== '')
    const problem = "The image could not be built: The command '/bin/sh -c exit 3' returned a non-zero code: 3"
    expect(recipes.buildOf(id)?.lines.at(-1)).toBe(problem)
    expect(statusOf(recipes.editing, recipes.buildOf(id), containers.resources.images)).toEqual({ tone: 'failed', text: `Failed: ${problem}` })
    expect(tagsOf(containers)).not.toContain('broken:1')
  })

  test('one removed goes from the list; the image it built stays', async () => {
    const { containers } = await opened()
    const recipes = containers.recipes
    recipes.compose()
    recipes.form!.draft = { tag: 'tools:1', dockerfile: DOCKERFILE }
    await recipes.build()
    await until(() => recipes.editing?.image !== '' && tagsOf(containers).includes('tools:1'))
    await recipes.remove()
    expect(recipes.form).toBeNull()
    expect(recipes.all).toEqual([])
    await containers.resources.load(['images'])
    expect(tagsOf(containers)).toContain('tools:1')
  })

  test('leaving the images closes the form', async () => {
    const { containers } = await opened({ recipes: [recipe('tools:1')] })
    containers.recipes.edit(containers.recipes.all[0]!)
    containers.select({ kind: 'volumes' })
    expect(containers.recipes.form).toBeNull()
  })
})
