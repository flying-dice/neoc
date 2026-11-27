import { afterAll, beforeEach, describe, expect, test } from 'bun:test'
import { startFakeDocker } from './fakeDocker'

// the fake answers in Docker's shapes, over HTTP, as a client reaches the real one
const docker = await startFakeDocker({
  containers: [{ name: 'web', image: 'nginx:1.27', running: true, ports: { '80/tcp': '8080' } }],
  networks: [{ name: 'blog_default', labels: { 'com.docker.compose.project': 'blog' } }],
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
