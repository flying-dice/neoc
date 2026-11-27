import { describe, expect, test } from 'bun:test'
import { ALL, Containers, IMAGES, NETWORKS, VOLUMES, stackScope } from './containers.svelte'
import { CONTAINERS, container, createFailingBackend, createFakeBackend, image, type FakeBackend } from './fakeApi'
import { blankEntry, blankMount, blankPort } from './spec'

/** A window on a fake backend, loaded and listening. */
async function opened(backend: FakeBackend = createFakeBackend()): Promise<Containers> {
  const containers = new Containers(backend.rpc, backend.events)
  containers.listen()
  await containers.load()
  return containers
}

/** Wait until `done()` holds (an event is answered by a call, on a later turn of the event loop). */
async function until(done: () => boolean): Promise<void> {
  for (let i = 0; i < 200 && !done(); i++) await Bun.sleep(2)
  expect(done()).toBe(true)
}

const named = (containers: Containers, name: string) => {
  const found = containers.all.find((c) => c.name === name)
  if (!found) throw new Error(`no container called ${name}`)
  return found
}
const stackNamed = (containers: Containers, name: string) => {
  const found = containers.stacks.find((s) => s.name === name)
  if (!found) throw new Error(`no stack called ${name}`)
  return found
}
const names = (containers: Containers): string[] => containers.containers.map((c) => c.name)
/** What the window holds in its state, as plain data. */
const plain = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T

describe('a window that has just opened', () => {
  test('says nothing about Docker until the first answer', () => {
    const backend = createFakeBackend()
    const containers = new Containers(backend.rpc, backend.events)
    expect(containers.loaded).toBe(false)
    expect(containers.all).toEqual([])
  })

  test('shows every container by name, and the stacks they belong to', async () => {
    const containers = await opened()
    expect(containers.loaded).toBe(true)
    expect(containers.engine).toEqual({ reachable: true, version: '27.3.1', host: '', problem: '' })
    expect(containers.title).toBe('All Containers')
    expect(names(containers)).toEqual(['blog-db-1', 'blog-web-1', 'media-jellyfin-1', 'media-transcoder-1', 'pihole', 'scratch'])
    expect(containers.stacks).toEqual([
      { name: 'blog', running: 2, total: 2, folder: '/srv/blog' },
      { name: 'media', running: 1, total: 2, folder: '/srv/media' }
    ])
    expect(containers.failure).toBeNull()
  })

  test('says why when Docker cannot be reached, and lists nothing', async () => {
    const containers = await opened(createFakeBackend({ problem: 'Docker is not running on this machine, or cannot be reached.' }))
    expect(containers.loaded).toBe(true)
    expect(containers.engine.reachable).toBe(false)
    expect(containers.engine.problem).toBe('Docker is not running on this machine, or cannot be reached.')
    expect(containers.all).toEqual([])
    // Docker being away is an answer, not a failed call
    expect(containers.failure).toBeNull()
  })

  test('keeps the reason when the backend itself cannot be reached', async () => {
    const containers = await opened(createFailingBackend('The host is not answering.'))
    expect(containers.failure).toBe('The host is not answering.')
    expect(containers.loaded).toBe(false)
  })
})

describe('a container', () => {
  test('is stopped, and started again', async () => {
    const containers = await opened()
    expect(await containers.act(named(containers, 'pihole'), 'stop')).toBe(true)
    expect(named(containers, 'pihole').state).toBe('exited')
    expect(await containers.act(named(containers, 'pihole'), 'start')).toBe(true)
    expect(named(containers, 'pihole').state).toBe('running')
  })

  test('is busy while the call is in flight, and not after', async () => {
    const containers = await opened(createFakeBackend({ pace: 5 }))
    const pihole = named(containers, 'pihole')
    const stopping = containers.act(pihole, 'stop')
    expect(containers.busy(pihole)).toBe(true)
    expect(containers.busy(named(containers, 'scratch'))).toBe(false)
    await stopping
    expect(containers.busy(named(containers, 'pihole'))).toBe(false)
  })

  test('is removed after the question, and gone from the list', async () => {
    const containers = await opened()
    containers.ask(named(containers, 'scratch'))
    expect(plain(containers.removal)).toMatchObject({ force: false, volumes: false, failure: null })
    await containers.remove()
    expect(containers.removal).toBeNull()
    expect(names(containers)).not.toContain('scratch')
  })

  test("that runs is removed only when forced: else Docker's reason stays with the question", async () => {
    const containers = await opened()
    containers.ask(named(containers, 'pihole'))
    await containers.remove()
    expect(containers.removal?.failure).toContain('container is running')
    expect(names(containers)).toContain('pihole')
    containers.removal!.force = true
    await containers.remove()
    expect(containers.removal).toBeNull()
    expect(names(containers)).not.toContain('pihole')
  })

  test("that Docker refuses keeps Docker's reason, stays as it was, and is not busy", async () => {
    const containers = await opened(createFakeBackend({ refusing: ['scratch'] }))
    expect(await containers.act(named(containers, 'scratch'), 'start')).toBe(false)
    expect(containers.failure).toContain('port is already allocated')
    expect(named(containers, 'scratch').state).toBe('exited')
    expect(containers.busy(named(containers, 'scratch'))).toBe(false)
    // the reason goes when a call works
    await containers.act(named(containers, 'pihole'), 'restart')
    expect(containers.failure).toBeNull()
  })
})

describe('a stack', () => {
  test('shows only its own containers', async () => {
    const containers = await opened()
    containers.select(stackScope('media'))
    expect(containers.title).toBe('media')
    expect(containers.stack?.folder).toBe('/srv/media')
    expect(names(containers)).toEqual(['media-jellyfin-1', 'media-transcoder-1'])
    containers.select(ALL)
    expect(containers.stack).toBeUndefined()
    expect(names(containers)).toHaveLength(6)
  })

  test('is started and stopped as one', async () => {
    const containers = await opened()
    await containers.actOnStack(stackNamed(containers, 'media'), 'start')
    expect(stackNamed(containers, 'media')).toMatchObject({ running: 2, total: 2 })
    await containers.actOnStack(stackNamed(containers, 'media'), 'stop')
    expect(stackNamed(containers, 'media')).toMatchObject({ running: 0, total: 2 })
    // the other stack was left alone
    expect(stackNamed(containers, 'blog')).toMatchObject({ running: 2, total: 2 })
  })

  test('makes each of its containers busy while a call for it is in flight', async () => {
    const containers = await opened(createFakeBackend({ pace: 5 }))
    const media = stackNamed(containers, 'media')
    const stopping = containers.actOnStack(media, 'stop')
    expect(containers.stackBusy(media)).toBe(true)
    expect(containers.busy(named(containers, 'media-jellyfin-1'))).toBe(true)
    expect(containers.busy(named(containers, 'blog-db-1'))).toBe(false)
    await stopping
    expect(containers.stackBusy(stackNamed(containers, 'media'))).toBe(false)
  })

  test('that is removed while it is shown gives way to every container', async () => {
    const containers = await opened()
    containers.select(stackScope('blog'))
    await containers.actOnStack(stackNamed(containers, 'blog'), 'remove')
    expect(containers.scope).toEqual(ALL)
    expect(containers.browsing).toBe(true)
    expect(containers.stacks.map((s) => s.name)).toEqual(['media'])
    expect(names(containers)).toEqual(['media-jellyfin-1', 'media-transcoder-1', 'pihole', 'scratch'])
  })

  test('with a container Docker refuses does the rest, and says why', async () => {
    const containers = await opened(createFakeBackend({ refusing: ['media-transcoder-1'] }))
    expect(await containers.actOnStack(stackNamed(containers, 'media'), 'stop')).toBe(false)
    expect(containers.failure).toContain('port is already allocated')
    // what the call left is read when Docker says it changed
    await containers.load()
    expect(named(containers, 'media-jellyfin-1').state).toBe('exited')
  })
})

describe('looking at a container', () => {
  test('shows its logs first, and what it was made with when asked', async () => {
    const containers = await opened()
    await containers.inspect(named(containers, 'blog-db-1'))
    expect(containers.inspected?.name).toBe('blog-db-1')
    expect(containers.tab).toBe('logs')
    expect(containers.logs?.text).toBe('blog-db-1 started\nlistening\n')
    expect(containers.following).toBe(true)
    await containers.show('details')
    expect(containers.details).toMatchObject({ command: 'docker-entrypoint.sh postgres', health: 'healthy', spec: { restart: 'unless-stopped' } })
    expect(containers.following).toBe(false)
  })

  test('reads new output when asked again', async () => {
    const backend = createFakeBackend()
    const containers = await opened(backend)
    await containers.inspect(named(containers, 'pihole'))
    backend.write('pihole', 'query: example.org')
    await containers.refresh()
    expect(containers.logs?.text).toEndWith('query: example.org\n')
  })

  test('a stopped container has logs, and no new output to wait for', async () => {
    const containers = await opened()
    await containers.inspect(named(containers, 'scratch'))
    expect(containers.logs).not.toBeNull()
    expect(containers.following).toBe(false)
  })

  test('ends when the container is removed, here or behind the app', async () => {
    const backend = createFakeBackend()
    const containers = await opened(backend)
    await containers.inspect(named(containers, 'scratch'))
    containers.ask(named(containers, 'scratch'))
    await containers.remove()
    expect(containers.inspected).toBeUndefined()
    expect(containers.logs).toBeNull()

    await containers.inspect(named(containers, 'pihole'))
    backend.outside((list) => list.filter((c) => c.name !== 'pihole'))
    await until(() => containers.inspected === undefined)
  })

  test('ends when another stack is picked, or the list is left', async () => {
    const containers = await opened()
    await containers.inspect(named(containers, 'pihole'))
    containers.select(stackScope('blog'))
    expect(containers.inspected).toBeUndefined()
    await containers.inspect(named(containers, 'blog-db-1'))
    containers.back()
    expect(containers.inspected).toBeUndefined()
    expect(containers.browsing).toBe(true)
  })

  test('an answer for a container that was left is dropped', async () => {
    const containers = await opened(createFakeBackend({ pace: 5 }))
    const reading = containers.inspect(named(containers, 'pihole'))
    containers.close()
    await reading
    expect(containers.logs).toBeNull()
  })
})

describe('the form', () => {
  test('for a new container starts empty, started once made, on bridge, with the networks and volumes to pick from', async () => {
    const containers = await opened()
    await containers.compose()
    expect(plain(containers.form)).toMatchObject({ mode: 'new', of: null, failure: null, saving: false })
    expect(plain(containers.form?.draft)).toMatchObject({ image: '', network: 'bridge', restart: 'no', start: true, ports: [], mounts: [] })
    expect(containers.savable).toBe(false)
    expect(containers.networks.map((n) => n.name)).toEqual(['blog_default', 'bridge', 'host', 'media_default', 'none'])
    expect(containers.volumes.map((v) => v.name)).toContain('pihole-data')
  })

  test('makes the container it says, pulling its image with the progress told, and looks at it', async () => {
    const backend = createFakeBackend({ pace: 2 })
    const containers = await opened(backend)
    await containers.compose()
    const form = containers.form!
    form.draft.image = 'nginx:1.27'
    form.draft.name = 'site'
    form.draft.environment.push({ ...blankEntry(), name: 'MODE', value: 'live' }, blankEntry())
    form.draft.ports.push({ ...blankPort(), host_port: '8088', container_port: '80' })
    form.draft.mounts.push({ ...blankMount('volume'), source: 'site-data', target: '/data' })
    const pulls: number[] = []
    backend.events.on('pulling', (progress) => pulls.push(progress.current))
    const saving = containers.save()
    expect(containers.form?.saving).toBe(true)
    expect(containers.savable).toBe(false)
    await saving
    expect(pulls).toEqual([0, 24_000_000, 48_000_000])
    expect(containers.form).toBeNull()
    expect(containers.inspected?.name).toBe('site')
    expect(named(containers, 'site')).toMatchObject({ image: 'nginx:1.27', state: 'running', ports: ['8088:80'] })
    await containers.show('details')
    // the empty row is left out
    expect(containers.details?.spec.environment).toEqual([{ name: 'MODE', value: 'live' }])
    expect(containers.volumes.map((v) => v.name)).not.toContain('site-data')
  })

  test("stays open with the backend's reason when it is refused, and nothing is made", async () => {
    const containers = await opened()
    await containers.compose()
    containers.form!.draft.image = 'nginx:1.27'
    containers.form!.draft.ports.push({ ...blankPort(), host_port: '99999', container_port: '80' })
    await containers.save()
    expect(containers.form?.failure).toBe("Port: the machine's port must be a number from 1 to 65535, or empty for one Docker chooses.")
    expect(containers.form?.saving).toBe(false)
    expect(containers.all).toHaveLength(CONTAINERS.length)
  })

  test('says why an image could not be pulled', async () => {
    const containers = await opened(createFakeBackend({ missingImages: ['nosuch/app:1'] }))
    await containers.compose()
    containers.form!.draft.image = 'nosuch/app:1'
    await containers.save()
    expect(containers.form?.failure).toContain('The image nosuch/app:1 could not be pulled')
  })

  test('to edit a container is filled from it, and saving makes it again by its new settings', async () => {
    const containers = await opened()
    await containers.open(named(containers, 'pihole'), 'edit')
    const form = containers.form!
    expect(plain(form)).toMatchObject({ mode: 'edit', of: { name: 'pihole' } })
    expect(plain(form.draft)).toMatchObject({ image: 'pihole/pihole:2024.07.0', name: 'pihole', start: true })
    expect(form.draft.ports.map((p) => `${p.host_port}:${p.container_port}/${p.protocol}`)).toEqual(['53:53/udp', '8053:80/tcp'])
    // a volume that is there is picked, not named anew
    expect(plain(form.draft.mounts[0])).toMatchObject({ kind: 'volume', source: 'pihole-data', fresh: false })
    form.draft.ports[1]!.host_port = '8054'
    await containers.save()
    expect(containers.form).toBeNull()
    expect(named(containers, 'pihole').ports).toEqual(['53:53/udp', '8054:80'])
    expect(containers.all).toHaveLength(CONTAINERS.length)
    expect(containers.inspected?.name).toBe('pihole')
  })

  test('to duplicate a container is filled from it, by a name no container has', async () => {
    const containers = await opened()
    await containers.open(named(containers, 'scratch'), 'duplicate')
    expect(plain(containers.form)).toMatchObject({ mode: 'duplicate', draft: { name: 'scratch-copy', image: 'alpine:3.20', start: true } })
    await containers.save()
    expect(names(containers)).toContain('scratch-copy')
    expect(names(containers)).toContain('scratch')
  })

  test('is left for where it was opened from', async () => {
    const containers = await opened()
    await containers.inspect(named(containers, 'pihole'))
    await containers.open(named(containers, 'pihole'), 'edit')
    containers.cancel()
    expect(containers.form).toBeNull()
    expect(containers.inspected?.name).toBe('pihole')
  })
})

describe('Docker changing behind the app', () => {
  test('a container made in a terminal appears', async () => {
    const backend = createFakeBackend()
    const containers = await opened(backend)
    backend.outside((list) => [...list, container('adhoc', 'nginx:1.27')])
    await until(() => names(containers).includes('adhoc'))
  })

  test('Docker going away empties the list and says why; coming back fills it again', async () => {
    const backend = createFakeBackend()
    const containers = await opened(backend)
    backend.reach(false)
    await until(() => !containers.engine.reachable)
    expect(containers.all).toEqual([])
    backend.reach(true)
    await until(() => containers.engine.reachable)
    expect(containers.all).toHaveLength(CONTAINERS.length)
  })

  test('nothing is heard after the window stops listening', async () => {
    const backend = createFakeBackend()
    const containers = new Containers(backend.rpc, backend.events)
    const stop = containers.listen()
    await containers.load()
    stop()
    backend.outside((list) => list.filter((c) => c.name !== 'scratch'))
    await Bun.sleep(10)
    expect(names(containers)).toContain('scratch')
  })

  test('the host connection coming back reads Docker again', async () => {
    const backend = createFakeBackend()
    const containers = await opened(backend)
    backend.outside((list) => list.filter((c) => c.name !== 'scratch'))
    await until(() => !names(containers).includes('scratch'))
    backend.emit('host:reconnected')
    await until(() => containers.failure === null && containers.all.length === 5)
  })
})

describe('choosing another Docker', () => {
  test('the address field starts as what is chosen', async () => {
    const containers = await opened(createFakeBackend({ host: 'tcp://nas.lan:2375', problem: 'Docker cannot be reached at tcp://nas.lan:2375.' }))
    expect(containers.hostDraft).toBe('tcp://nas.lan:2375')
  })

  test('an address nothing listens at is chosen all the same, and said to be unreachable', async () => {
    const containers = await opened()
    containers.hostDraft = '  tcp://nas.lan:2375 '
    await containers.connect()
    expect(containers.engine).toMatchObject({ reachable: false, host: 'tcp://nas.lan:2375', problem: 'Docker cannot be reached at tcp://nas.lan:2375.' })
    expect(containers.hostDraft).toBe('tcp://nas.lan:2375')
    expect(containers.failure).toBeNull()
  })

  test('an empty address is this machine again', async () => {
    const containers = await opened(createFakeBackend({ host: 'tcp://nas.lan:2375', problem: 'Docker cannot be reached at tcp://nas.lan:2375.' }))
    containers.hostDraft = ''
    await containers.connect()
    expect(containers.engine).toMatchObject({ reachable: true, host: '' })
    expect(containers.all).toHaveLength(CONTAINERS.length)
  })

  test('something that is not an address is refused, and what was typed stays', async () => {
    const containers = await opened()
    containers.hostDraft = 'nas.lan'
    await containers.connect()
    expect(containers.failure).toContain('must start with unix:///')
    expect(containers.hostDraft).toBe('nas.lan')
    expect(containers.engine.host).toBe('')
  })
})

describe('the images', () => {
  const imageNamed = (containers: Containers, tag: string) => {
    const found = containers.resources.images.find((i) => i.tags.includes(tag))
    if (!found) throw new Error(`no image called ${tag}`)
    return found
  }

  test('are read with the window, each with the containers made from it, the dangling one last', async () => {
    const containers = await opened()
    containers.select(IMAGES)
    expect(containers.title).toBe('Images')
    const images = containers.resources.images
    expect(images.map((i) => i.tags[0] ?? '<none>')).toEqual([
      'alpine:3.20',
      'ghost:5',
      'jellyfin/jellyfin:10.9',
      'linuxserver/ffmpeg:7',
      'pihole/pihole:2024.07.0',
      'postgres:16',
      'redis:7',
      '<none>'
    ])
    expect(imageNamed(containers, 'postgres:16').containers).toEqual(['blog-db-1'])
    expect(imageNamed(containers, 'redis:7').containers).toEqual([])
  })

  test('are pulled by name, with the pull told as it goes, and the name is left empty for the next', async () => {
    const backend = createFakeBackend({ pace: 2 })
    const containers = await opened(backend)
    // what the list holds of the pull each time the backend tells how it goes (the window hears it first)
    const told: (number | undefined)[] = []
    backend.events.on('pulling', () => told.push(containers.resources.pull.progress?.current))
    containers.resources.pull.reference = ' nginx:1.27 '
    const pulling = containers.resources.pullImage()
    expect(containers.resources.pull.busy).toBe(true)
    await pulling
    expect(told).toEqual([0, 24_000_000, 48_000_000])
    expect(containers.resources.pull).toMatchObject({ reference: '', busy: false, progress: null, failure: null })
    expect(imageNamed(containers, 'nginx:1.27').size).toBe(48_000_000)
  })

  test("that cannot be pulled keep the name typed, with the backend's reason", async () => {
    const containers = await opened(createFakeBackend({ missingImages: ['nosuch/app:1'] }))
    containers.resources.pull.reference = 'nosuch/app:1'
    await containers.resources.pullImage()
    expect(containers.resources.pull.failure).toContain('The image nosuch/app:1 could not be pulled')
    expect(containers.resources.pull.reference).toBe('nosuch/app:1')
    containers.resources.pull.reference = 'Not An Image'
    await containers.resources.pullImage()
    expect(containers.resources.pull.failure).toContain('is not the name of an image')
  })

  test('one no container was made from is removed after the question', async () => {
    const containers = await opened()
    containers.resources.askImage(imageNamed(containers, 'redis:7'))
    expect(plain(containers.resources.removal)).toMatchObject({ kind: 'image', name: 'redis:7', users: [], force: false })
    await containers.resources.remove()
    expect(containers.resources.removal).toBeNull()
    expect(containers.resources.images.some((i) => i.tags.includes('redis:7'))).toBe(false)
  })

  test("one a stopped container was made from goes only when forced; one a running container runs, not even then", async () => {
    const containers = await opened()
    containers.resources.askImage(imageNamed(containers, 'alpine:3.20'))
    await containers.resources.remove()
    expect(containers.resources.removal?.failure).toContain('(must be forced) - image is being used by stopped container')
    containers.resources.removal!.force = true
    await containers.resources.remove()
    expect(containers.resources.removal).toBeNull()
    containers.resources.askImage(imageNamed(containers, 'postgres:16'))
    containers.resources.removal!.force = true
    await containers.resources.remove()
    expect(containers.resources.removal?.failure).toContain('(cannot be forced)')
  })

  test('the dangling ones are pruned, and the window says what that freed', async () => {
    const containers = await opened()
    await containers.resources.prune('images')
    expect(containers.resources.report).toEqual({ kind: 'images', text: 'Removed 1 dangling image, and freed 140 MB.' })
    expect(containers.resources.images.every((i) => i.tags.length > 0)).toBe(true)
    await containers.resources.prune('images')
    expect(containers.resources.report?.text).toBe('There were no dangling images.')
    // what the prune said goes when the list is left
    containers.select(ALL)
    expect(containers.resources.report).toBeNull()
  })

  test('make a new container from one: the form, with its image filled in', async () => {
    const containers = await opened()
    containers.select(IMAGES)
    await containers.compose(imageNamed(containers, 'redis:7').tags[0])
    expect(containers.form?.draft.image).toBe('redis:7')
    expect(containers.form?.mode).toBe('new')
  })

  test('are read again when Docker changes, as the containers are', async () => {
    const backend = createFakeBackend({ images: [image(['alpine:3.20'])] })
    let reads = 0
    const images = backend.rpc.images
    backend.rpc.images = () => {
      reads += 1
      return images()
    }
    const containers = await opened(backend)
    expect(reads).toBe(1)
    expect(containers.resources.images).toHaveLength(1)
    backend.emit('changed')
    await until(() => reads === 2)
  })
})

describe('the volumes', () => {
  test('are read with the containers that use them', async () => {
    const containers = await opened()
    containers.select(VOLUMES)
    expect(containers.title).toBe('Volumes')
    expect(containers.resources.volumes.find((v) => v.name === 'pihole-data')).toMatchObject({ driver: 'local', containers: ['pihole'] })
  })

  test('one is made with its name, driver and labels, an empty label row left out', async () => {
    const containers = await opened()
    containers.resources.composeVolume()
    const form = containers.resources.volumeForm!
    expect(plain(form.draft)).toEqual({ name: '', driver: 'local', labels: [] })
    expect(containers.resources.savable).toBe(true)
    form.draft.name = 'cache'
    form.draft.labels.push({ ...blankEntry(), name: 'team', value: 'web' }, blankEntry())
    await containers.resources.save()
    expect(containers.resources.volumeForm).toBeNull()
    expect(containers.resources.volumes.find((v) => v.name === 'cache')).toMatchObject({ containers: [] })
  })

  test("a name another volume has is refused, and the form stays with the backend's reason", async () => {
    const containers = await opened()
    containers.resources.composeVolume()
    containers.resources.volumeForm!.draft.name = 'pihole-data'
    await containers.resources.save()
    expect(containers.resources.volumeForm?.failure).toBe('There is a volume named pihole-data already.')
  })

  test('one a container uses is refused in words that name it; an unused one goes', async () => {
    const containers = await opened()
    const used = containers.resources.volumes.find((v) => v.name === 'pihole-data')!
    containers.resources.askVolume(used)
    expect(containers.resources.removal?.users).toEqual(['pihole'])
    await containers.resources.remove()
    expect(containers.resources.removal?.failure).toBe('The volume pihole-data is used by pihole: Docker removes a volume only once no container uses it, running or not.')
    containers.resources.removal = null
    containers.resources.composeVolume()
    containers.resources.volumeForm!.draft.name = 'spare'
    await containers.resources.save()
    containers.resources.askVolume(containers.resources.volumes.find((v) => v.name === 'spare')!)
    await containers.resources.remove()
    expect(containers.resources.volumes.some((v) => v.name === 'spare')).toBe(false)
  })

  test('the unused ones are pruned with their data, and the window says which', async () => {
    const containers = await opened()
    containers.resources.composeVolume()
    containers.resources.volumeForm!.draft.name = 'spare'
    await containers.resources.save()
    await containers.resources.prune('volumes')
    expect(containers.resources.report).toEqual({ kind: 'volumes', text: 'Deleted spare with its data, and freed 24.0 MB.' })
    expect(containers.resources.volumes.every((v) => v.containers.length > 0)).toBe(true)
  })
})

describe('the networks', () => {
  test("are read with their addresses and the containers on them; Docker's own are said to be", async () => {
    const containers = await opened()
    containers.select(NETWORKS)
    expect(containers.title).toBe('Networks')
    expect(containers.resources.networks.find((n) => n.name === 'blog_default')).toMatchObject({
      subnet: '172.18.0.0/16',
      gateway: '172.18.0.1',
      predefined: false,
      containers: ['blog-db-1', 'blog-web-1']
    })
    expect(containers.resources.networks.filter((n) => n.predefined).map((n) => n.name)).toEqual(['bridge', 'host', 'none'])
  })

  test('one is made with a subnet, a gateway and its switches, and can be put a container on', async () => {
    const containers = await opened()
    containers.resources.composeNetwork()
    expect(containers.resources.savable).toBe(false)
    const form = containers.resources.networkForm!
    Object.assign(form.draft, { name: 'backend', subnet: '172.30.0.0/16', gateway: '172.30.0.1', internal: true })
    expect(containers.resources.savable).toBe(true)
    await containers.resources.save()
    expect(containers.resources.networkForm).toBeNull()
    expect(containers.resources.networks.find((n) => n.name === 'backend')).toMatchObject({ subnet: '172.30.0.0/16', gateway: '172.30.0.1', internal: true })
    await containers.compose()
    expect(containers.networks.map((n) => n.name)).toContain('backend')
  })

  test("a subnet that is none is refused, in the backend's words", async () => {
    const containers = await opened()
    containers.resources.composeNetwork()
    Object.assign(containers.resources.networkForm!.draft, { name: 'backend', subnet: 'everything' })
    await containers.resources.save()
    expect(containers.resources.networkForm?.failure).toContain('everything is not a subnet')
  })

  test("Docker's own are not asked about; one a running container is on is refused, naming it", async () => {
    const containers = await opened()
    containers.resources.askNetwork(containers.resources.networks.find((n) => n.name === 'bridge')!)
    expect(containers.resources.removal).toBeNull()
    containers.resources.askNetwork(containers.resources.networks.find((n) => n.name === 'blog_default')!)
    await containers.resources.remove()
    expect(containers.resources.removal?.failure).toBe('blog-db-1 and blog-web-1 run on the network blog_default: Docker removes a network only once no running container is on it.')
    await containers.actOnStack(stackNamed(containers, 'blog'), 'stop')
    await containers.resources.remove()
    expect(containers.resources.removal).toBeNull()
    expect(containers.resources.networks.some((n) => n.name === 'blog_default')).toBe(false)
  })
})
