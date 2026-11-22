/**
 * A fake of the host's management methods for stories and component tests: installed apps, a registry catalog,
 * app refs to preview, and paired devices, with the same answers and error messages the real host gives. It
 * keeps its own state, so install / disable / uninstall / revoke behave end to end. The real host is exercised
 * by the Playwright journeys.
 */
import { HostError } from './channel'
import type { AgentRow, AppPreview, AppSummary, Catalog, DeviceRow, HostApi } from './hostApi'

export const FAKE_CALCULATOR: AppSummary = {
  id: 'calculator',
  name: 'Calculator',
  version: '0.1.0',
  state: 'running',
  icon: 'calculator',
  tint: 'orange',
  description: 'Arithmetic for you and your agents.',
  window: { w: 340, h: 560, minW: 320, minH: 500 },
  source: 'this computer',
  client: false,
  permissions: []
}

/** The Agents app: an ordinary installed app, which a host may or may not have (stories show both). */
export const FAKE_AGENTS_APP: AppSummary = {
  id: 'agents',
  name: 'Agents',
  version: '0.1.0',
  state: 'running',
  icon: 'sparkles',
  tint: 'purple',
  description: 'Chat with the agents on this host.',
  window: { w: 520, h: 640, minW: 480, minH: 420 },
  source: 'this computer',
  client: false,
  permissions: []
}

const NOTES: AppPreview = {
  url: 'file:///registry/notes/0.2.0/app-ref.json',
  id: 'notes',
  name: 'Notes',
  version: '0.2.0',
  icon: 'doc',
  tint: 'yellow',
  description: 'Plain notes your agents can read and append to.',
  source: 'this computer',
  sha256: '0'.repeat(64)
}

const asPreview = (a: AppSummary, url: string): AppPreview => ({
  url,
  id: a.id,
  name: a.name,
  version: a.version,
  icon: a.icon,
  tint: a.tint,
  description: a.description,
  source: a.source,
  installed: { version: a.version, state: a.state },
  sha256: '0'.repeat(64)
})

export const FAKE_AGENT: AgentRow = {
  id: 'scripted',
  name: 'Scripted agent',
  role: 'Answers by script, for tests',
  avatar: 'bolt',
  runtime: { command: 'scripted-agent', args: [], envKeys: [] },
  grants: { all: false, apps: [] },
  approval: 'auto_read_only'
}

export function fakeDevices(now = Date.now()): DeviceRow[] {
  return [
    { id: 'd1', name: 'Safari on iPad', createdAt: now - 9e8, lastSeenAt: now, current: true },
    {
      id: 'd2',
      name: 'Chrome on Mac',
      createdAt: now - 5e8,
      lastSeenAt: now - 3 * 3_600_000,
      current: false
    },
    {
      id: 'd3',
      name: 'Firefox on Linux',
      createdAt: now - 2e8,
      lastSeenAt: now - 2 * 86_400_000,
      current: false
    }
  ]
}

export interface FakeHost extends HostApi {
  apps: AppSummary[]
  devices: DeviceRow[]
  agents: AgentRow[]
  /** Methods called, in order, with their params. */
  calls: [string, Record<string, unknown>][]
  /** Push a host notification to the shell (`window.open`, `app.installed`, ...). */
  emit(method: string, params?: Record<string, unknown>): void
}

export function createFakeHost(
  options: {
    apps?: AppSummary[]
    devices?: DeviceRow[]
    agents?: AgentRow[]
    registryErrors?: string[]
  } = {}
): FakeHost {
  const listeners = new Set<(method: string, params: Record<string, unknown>) => void>()
  const fake: FakeHost = {
    apps: [...(options.apps ?? [FAKE_CALCULATOR])],
    devices: [...(options.devices ?? fakeDevices())],
    agents: [...(options.agents ?? [FAKE_AGENT])],
    calls: [],
    emit: (method, params = {}) => listeners.forEach((l) => l(method, params)),
    onNotification(listener) {
      listeners.add(listener)
      return () => void listeners.delete(listener)
    },
    async call(method: string, params: Record<string, unknown> = {}): Promise<unknown> {
      fake.calls.push([method, params])
      const app = (): AppSummary => {
        const found = fake.apps.find((a) => a.id === params.app)
        if (!found) throw new HostError(-32002, `not found: ${String(params.app)}`)
        return found
      }
      switch (method) {
        case 'app.list':
          return { apps: fake.apps }
        case 'app.available': {
          const offered = [
            NOTES,
            ...fake.apps.map((a) => asPreview(a, `file:///registry/${a.id}/app-ref.json`))
          ]
          const apps = offered.map((p) => {
            const installed = fake.apps.find((a) => a.id === p.id)
            return installed
              ? { ...p, installed: { version: installed.version, state: installed.state } }
              : p
          })
          return { apps, errors: options.registryErrors ?? [] } satisfies Catalog
        }
        case 'app.preview': {
          const url = String(params.url)
          if (url.includes('missing')) throw new HostError(-32002, `not found: ${url}`)
          if (url.includes('bad')) throw new HostError(-32003, `rejected: bad app ref at ${url}`)
          const installed = fake.apps.find((a) => a.id === NOTES.id)
          return {
            ...NOTES,
            url,
            ...(installed
              ? { installed: { version: installed.version, state: installed.state } }
              : {})
          }
        }
        case 'app.install': {
          if (fake.apps.some((a) => a.id === NOTES.id))
            throw new HostError(-32004, 'app `notes` is already installed')
          fake.apps.push({
            ...NOTES,
            state: 'running',
            window: { w: 960, h: 640, minW: 480, minH: 360 },
            client: false,
            permissions: []
          } as AppSummary)
          fake.emit('app.installed', { app: NOTES.id })
          return { app: NOTES.id }
        }
        case 'app.uninstall': {
          app()
          fake.apps = fake.apps.filter((a) => a.id !== params.app)
          fake.emit('app.uninstalled', { app: params.app })
          return null
        }
        case 'app.enable':
        case 'app.disable': {
          const target = app()
          target.state = method === 'app.enable' ? 'running' : 'disabled'
          fake.emit(method === 'app.enable' ? 'app.enabled' : 'app.disabled', { app: target.id })
          return null
        }
        case 'app.reload':
          app().state = 'running'
          return null
        case 'agents.list':
          return { agents: fake.agents }
        case 'host.devices.list':
          return { devices: fake.devices }
        case 'host.devices.revoke':
          fake.devices = fake.devices.filter((d) => d.id !== params.id)
          return null
        default:
          throw new HostError(-32601, `method \`${method}\` not found`)
      }
    }
  }
  return fake
}
