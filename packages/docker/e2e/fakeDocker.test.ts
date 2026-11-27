import { afterAll, beforeEach, describe, expect, test } from 'bun:test'
import { startFakeDocker } from './fakeDocker'

// the fake answers in Docker's shapes, over HTTP, as a client reaches the real one
const docker = await startFakeDocker({
  containers: [{ name: 'web', image: 'nginx:1.27', running: true, ports: { '80/tcp': '8080' } }],
  networks: [{ name: 'blog_default', labels: { 'com.docker.compose.project': 'blog' } }],
  images: [{ tags: [], size: 70_000_000 }, { tags: ['redis:7', 'redis:latest'] }],
  missingImages: ['nosuch/app:1']
})
const base = docker.host.replace('tcp://', 'http://')
/** One call: its status, and its answer read as JSON (`text` for a stream of it). */
const call = async (method: string, path: string, body?: unknown): Promise<{ status: number; text: string; body: any }> => {
  const res = await fetch(`${base}${path}`, {
    method,
    headers: { 'content-type': 'application/json' },
    ...(body === undefined ? {} : { body: JSON.stringify(body) })
  })
  const text = await res.text()
  let json: unknown = null
  try {
    json = JSON.parse(text)
  } catch {
    // empty, or a stream of answers
  }
  return { status: res.status, text, body: json }
}
const pull = (image: string, tag: string) => call('POST', `/images/create?fromImage=${image}&tag=${tag}`)

beforeEach(() => docker.reset())
afterAll(() => docker.close())

describe('a pull', () => {
  test('reports each layer as Docker does, and keeps the image', async () => {
    const { status, text } = await pull('nginx', '1.28')
    expect(status).toBe(200)
    const steps = text.trim().split('\n').map((line) => JSON.parse(line) as { status: string; progressDetail?: { total: number } })
    expect(steps.some((s) => s.status === 'Downloading' && s.progressDetail?.total === 3_000_000)).toBe(true)
    expect(steps.at(-1)?.status).toBe('Status: Downloaded newer image for nginx:1.28')
    expect(docker.pulled()).toEqual(['nginx:1.28'])
  })

  test('of an image the registry does not have is refused in Docker words', async () => {
    const { status, body } = await pull('nosuch/app', '1')
    expect(status).toBe(404)
    expect(body.message).toContain('pull access denied for nosuch/app')
    expect(docker.pulled()).toEqual([])
  })
})

describe('a container that is made', () => {
  test('keeps its bindings, mounts, network and restart policy, and inspects as Docker does', async () => {
    await pull('nginx', '1.28')
    const made = await call('POST', '/containers/create?name=site', {
      Image: 'nginx:1.28',
      Env: ['A=1'],
      HostConfig: {
        PortBindings: { '80/tcp': [{ HostIp: '127.0.0.1', HostPort: '8081' }], '443/tcp': [{ HostPort: '' }] },
        Mounts: [
          { Type: 'bind', Source: '/srv/www', Target: '/usr/share/nginx/html', ReadOnly: true },
          { Type: 'volume', Source: 'site-data', Target: '/data' },
          { Type: 'volume', Target: '/cache' }
        ],
        NetworkMode: 'blog_default',
        RestartPolicy: { Name: 'on-failure', MaximumRetryCount: 3 }
      }
    })
    expect(made.status).toBe(201)
    expect(docker.container('site')).toMatchObject({ network: 'blog_default', restart: 'on-failure', retries: 3, ports: { '80/tcp': '8081', '443/tcp': '49153' } })
    // a named volume is made with it, and so is an anonymous one
    expect(docker.volumes()).toContain('site-data')
    expect(docker.volumes()).toHaveLength(3)

    const inspected = (await call('GET', '/containers/site/json')).body
    expect(inspected.HostConfig.PortBindings['80/tcp']).toEqual([{ HostIp: '127.0.0.1', HostPort: '8081' }])
    expect(inspected.HostConfig.RestartPolicy).toEqual({ Name: 'on-failure', MaximumRetryCount: 3 })
    expect(inspected.Config.Env).toEqual(['A=1'])
    expect(inspected.Config.Hostname).toBe(made.body.Id.slice(0, 12))
    expect(inspected.Mounts.find((m: { Destination: string }) => m.Destination === '/usr/share/nginx/html')).toMatchObject({ Type: 'bind', RW: false })

    // removing it with its volumes takes its anonymous one, never a named one
    expect((await call('DELETE', '/containers/site?v=true')).status).toBe(204)
    expect(docker.volumes()).toContain('site-data')
    expect(docker.volumes()).toHaveLength(2)
  })

  test('is refused what Docker refuses: a missing image, an unknown network, a relative path, a name in use', async () => {
    expect((await call('POST', '/containers/create?name=x1', { Image: 'nginx:9' })).body.message).toBe('No such image: nginx:9')
    await pull('nginx', '1.28')
    const refused = async (body: object, name = 'x1'): Promise<string> => (await call('POST', `/containers/create?name=${name}`, { Image: 'nginx:1.28', ...body })).body.message
    expect(await refused({ HostConfig: { NetworkMode: 'nowhere' } })).toBe('network nowhere not found')
    expect(await refused({ HostConfig: { Mounts: [{ Type: 'bind', Source: 'www', Target: '/x' }] } })).toContain('must be absolute')
    expect(await refused({ HostConfig: { RestartPolicy: { Name: 'always', MaximumRetryCount: 2 } } })).toContain("only be used with 'on-failure'")
    expect(await refused({}, 'web')).toContain('is already in use')
    expect(docker.count()).toBe(1)
  })
})

describe('a running container', () => {
  test('is removed only when forced', async () => {
    const refused = await call('DELETE', '/containers/web')
    expect(refused.status).toBe(409)
    expect(refused.body.message).toContain('container is running')
    expect((await call('DELETE', '/containers/web?force=true')).status).toBe(204)
    expect(docker.container('web')).toBeUndefined()
  })

  test("holds its machine's port: another container that publishes it does not start", async () => {
    await pull('nginx', '1.28')
    await call('POST', '/containers/create?name=copy', { Image: 'nginx:1.28', HostConfig: { PortBindings: { '80/tcp': [{ HostPort: '8080' }] } } })
    const started = await call('POST', '/containers/copy/start')
    expect(started.status).toBe(500)
    expect(started.body.message).toContain('Bind for 0.0.0.0:8080 failed: port is already allocated')
  })
})

test("Docker's own networks are there, and cannot be removed", async () => {
  expect(docker.networks()).toEqual(['bridge', 'host', 'none', 'blog_default'])
  expect((await call('DELETE', '/networks/bridge')).status).toBe(403)
  const volumes = (await call('GET', '/volumes')).body
  expect(volumes.Volumes.map((v: { Name: string }) => v.Name)).toEqual(['web-data'])
})

/** An event stream the test reads: every line that arrives, as Docker's JSON. */
async function listen(filters: object): Promise<{ events: { Type: string; Action: string }[]; close(): void }> {
  const controller = new AbortController()
  const res = await fetch(`${base}/events?filters=${encodeURIComponent(JSON.stringify(filters))}`, { signal: controller.signal })
  const events: { Type: string; Action: string }[] = []
  const reader = res.body!.getReader()
  void (async () => {
    let text = ''
    for (;;) {
      const { value, done } = await reader.read().catch(() => ({ value: undefined, done: true }))
      if (done) return
      text += new TextDecoder().decode(value)
      const lines = text.split('\n')
      text = lines.pop() ?? ''
      for (const line of lines) if (line !== '') events.push(JSON.parse(line))
    }
  })()
  return { events, close: () => controller.abort() }
}

describe('the images', () => {
  test('are listed as Docker lists them: an id each, sizes, dangling ones without names, and no count of containers', async () => {
    const listed = (await call('GET', '/images/json')).body as { Id: string; RepoTags: string[]; Size: number; Containers: number }[]
    expect(listed.map((i) => i.RepoTags)).toEqual([['nginx:1.27'], [], ['redis:7', 'redis:latest']])
    expect(new Set(listed.map((i) => i.Id)).size).toBe(3)
    expect(listed.every((i) => /^sha256:[0-9a-f]{64}$/.test(i.Id) && i.Size > 0 && i.Containers === -1)).toBe(true)
    expect(listed[1]!.Size).toBe(70_000_000)
    const dangling = (await call('GET', `/images/json?filters=${encodeURIComponent('{"dangling":["true"]}')}`)).body
    expect(dangling).toHaveLength(1)
    // a container says which image it was made from
    const [web] = (await call('GET', '/containers/json?all=true')).body
    expect(web.ImageID).toBe(listed[0]!.Id)
    expect(web.Image).toBe('nginx:1.27')
  })

  test('one a container was made from is removed only when forced, and one a running container runs not even then', async () => {
    const nginx = docker.images()[0]!
    const byName = await call('DELETE', '/images/nginx:1.27')
    expect(byName.status).toBe(409)
    expect(byName.body.message).toContain('(must force) - container')
    const byId = await call('DELETE', `/images/${nginx.id}?force=1`)
    expect(byId.status).toBe(409)
    expect(byId.body.message).toContain('(cannot be forced) - image is being used by running container')
    // stopped, it goes when forced: its names, then itself
    await call('POST', '/containers/web/stop')
    expect((await call('DELETE', `/images/${nginx.id}`)).body.message).toContain('(must be forced) - image is being used by stopped container')
    expect((await call('DELETE', `/images/${nginx.id}?force=true`)).body).toEqual([{ Untagged: 'nginx:1.27' }, { Deleted: nginx.id }])
    expect(docker.images().map((i) => i.id)).not.toContain(nginx.id)
  })

  test('one with two names loses one name at a time, and goes with its last', async () => {
    expect((await call('DELETE', '/images/redis:latest')).body).toEqual([{ Untagged: 'redis:latest' }])
    const redis = docker.images().find((i) => i.tags.includes('redis:7'))!
    expect((await call('DELETE', '/images/redis:7')).body).toEqual([{ Untagged: 'redis:7' }, { Deleted: redis.id }])
    expect((await call('DELETE', '/images/redis:7')).status).toBe(404)
  })

  test('a pull of a name that is there is up to date, and a prune takes the dangling ones and says what it freed', async () => {
    const watching = await listen({ type: ['image'] })
    const { text } = await pull('nginx', '1.27')
    expect(text).toContain('Status: Image is up to date for nginx:1.27')
    expect(docker.images()).toHaveLength(3)
    const pruned = await call('POST', '/images/prune')
    expect(pruned.body.SpaceReclaimed).toBe(70_000_000)
    // the image, and a layer of it
    expect(pruned.body.ImagesDeleted).toHaveLength(2)
    expect(docker.images().map((i) => i.tags)).toEqual([['nginx:1.27'], ['redis:7', 'redis:latest']])
    expect((await call('POST', '/images/prune')).body).toEqual({ ImagesDeleted: null, SpaceReclaimed: 0 })
    await Bun.sleep(50)
    expect(watching.events.map((e) => e.Action)).toEqual(['pull', 'delete', 'prune', 'prune'])
    watching.close()
  })
})

describe('the volumes', () => {
  test('are made with their driver and labels, and one that is there is the answer', async () => {
    const made = await call('POST', '/volumes/create', { Name: 'cache', Labels: { team: 'web' } })
    expect(made.status).toBe(201)
    expect(made.body).toMatchObject({ Name: 'cache', Driver: 'local', Labels: { team: 'web' } })
    expect((await call('POST', '/volumes/create', { Name: 'cache' })).body.Labels).toEqual({ team: 'web' })
    expect((await call('POST', '/volumes/create', { Name: 'shared', Driver: 'nfs' })).body.message).toContain('plugin "nfs" not found')
    expect((await call('POST', '/volumes/create', { Name: 'a b' })).status).toBe(400)
    expect(docker.volume('cache')).toEqual({ name: 'cache', driver: 'local', labels: { team: 'web' } })
  })

  test('one a container mounts is not removed, running or not; a prune of all takes the others and their data', async () => {
    await call('POST', '/volumes/create', { Name: 'cache' })
    const refused = await call('DELETE', '/volumes/web-data?force=true')
    expect(refused.status).toBe(409)
    expect(refused.body.message).toMatch(/^remove web-data: volume is in use - \[[0-9a-f]{64}\]$/)
    // a prune without `all` takes only the anonymous ones, of which there are none
    expect((await call('POST', '/volumes/prune')).body).toEqual({ VolumesDeleted: null, SpaceReclaimed: 0 })
    const pruned = await call('POST', `/volumes/prune?filters=${encodeURIComponent('{"all":["true"]}')}`)
    expect(pruned.body).toEqual({ VolumesDeleted: ['cache'], SpaceReclaimed: 0 })
    expect(docker.volumes()).toEqual(['web-data'])
    expect((await call('DELETE', '/volumes/cache')).status).toBe(404)
  })
})

describe('the networks', () => {
  test('are listed with their IPAM, made with a subnet or one Docker gives, and refused one that overlaps', async () => {
    const listed = (await call('GET', '/networks')).body as { Name: string; IPAM: { Config: { Subnet: string; Gateway: string }[] } }[]
    expect(listed.find((n) => n.Name === 'bridge')!.IPAM.Config).toEqual([{ Subnet: '172.17.0.0/16', Gateway: '172.17.0.1' }])
    expect(listed.find((n) => n.Name === 'host')!.IPAM.Config).toEqual([])
    expect(listed.find((n) => n.Name === 'blog_default')!.IPAM.Config).toEqual([{ Subnet: '172.18.0.0/16', Gateway: '172.18.0.1' }])

    const made = await call('POST', '/networks/create', { Name: 'lan', Driver: 'macvlan', Internal: true, IPAM: { Config: [{ Subnet: '10.20.0.0/24' }] }, Labels: { a: '1' } })
    expect(made.status).toBe(201)
    expect(docker.network('lan')).toEqual({ name: 'lan', driver: 'macvlan', subnet: '10.20.0.0/24', gateway: '10.20.0.1', internal: true, attachable: false, labels: { a: '1' } })
    await call('POST', '/networks/create', { Name: 'next' })
    expect(docker.network('next')?.subnet).toBe('172.19.0.0/16')
    expect((await call('POST', '/networks/create', { Name: 'lan' })).status).toBe(409)
    expect((await call('POST', '/networks/create', { Name: 'clash', IPAM: { Config: [{ Subnet: '10.20.0.128/25' }] } })).body.message).toBe(
      'Pool overlaps with other one on this address space'
    )
    expect((await call('POST', '/networks/create', { Name: 'swarm', Driver: 'overlay' })).status).toBe(503)
  })

  test('one a running container is on is not removed; once it stops, it is', async () => {
    await pull('nginx', '1.28')
    await call('POST', '/networks/create', { Name: 'backend' })
    await call('POST', '/containers/create?name=api', { Image: 'nginx:1.28', HostConfig: { NetworkMode: 'backend' } })
    await call('POST', '/containers/api/start')
    const [api] = ((await call('GET', '/containers/json')).body as { Names: string[]; NetworkSettings: { Networks: object } }[]).filter((c) => c.Names[0] === '/api')
    expect(Object.keys(api!.NetworkSettings.Networks)).toEqual(['backend'])
    const refused = await call('DELETE', '/networks/backend')
    expect(refused.status).toBe(403)
    expect(refused.body.message).toContain('has active endpoints')
    await call('POST', '/containers/api/stop')
    expect((await call('DELETE', '/networks/backend')).status).toBe(204)
    expect(docker.networks()).not.toContain('backend')
  })
})

test('an event reaches the listeners that asked for its type, its action and its labels', async () => {
  const containers = await listen({ type: ['container'] })
  const volumes = await listen({ type: ['volume'], event: ['create'] })
  const labelled = await listen({ type: ['container'], label: ['app=other'] })
  await call('POST', '/volumes/create', { Name: 'cache' })
  await call('POST', '/containers/web/stop')
  await Bun.sleep(50)
  expect(containers.events.map((e) => `${e.Type} ${e.Action}`)).toEqual(['container die'])
  expect(volumes.events.map((e) => `${e.Type} ${e.Action}`)).toEqual(['volume create'])
  expect(labelled.events, 'web has no such label').toEqual([])
  for (const stream of [containers, volumes, labelled]) stream.close()
})
