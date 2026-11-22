/**
 * A fake of the host's management methods for stories and component tests: installed apps, a registry catalog,
 * app refs to preview, paired devices, and the Documents apps keep, with the same answers and error messages the real
 * host gives. It keeps its own state, so install / disable / uninstall / revoke / remove behave end to end. The real host is exercised
 * by the Playwright journeys.
 */
import { HostError } from './channel'
import type { AppPreview, AppSummary, Catalog, DeviceRow, FileEntry, HostApi } from './hostApi'
import type { Uploader } from './upload'

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
  permissions: []
}

/** A second installed app, for stories that need more than one. */
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

function fakeDevices(now = Date.now()): DeviceRow[] {
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

/**
 * A folder of fake files: a name to a file's size in bytes, to the folder under that name, or to a sentence: a folder
 * the host may not open, which is what listing it answers.
 */
export interface FakeFolder {
  [name: string]: number | string | FakeFolder
}

/** When every fake file was last changed: a fixed time, so a story draws the same on any day. */
const FAKE_MODIFIED = Date.UTC(2026, 5, 8, 13, 5)

/** What the Agents app keeps, for the Files stories: folders, files of a few sizes, a long name. */
export const FAKE_DOCUMENTS: Record<string, FakeFolder> = {
  agents: {
    transcripts: { 'research-2026-06-08.md': 18_240, 'coder-2026-06-07.md': 4_096 },
    exports: {},
    'refactor-plan.md': 2_310,
    'uptime-report.csv': 148_000,
    'a-very-long-file-name-that-an-agent-chose-without-thinking-about-narrow-windows.txt': 12
  }
}

/** The fake host's own file system, for the Files stories: a home directory, and around it a little of a machine. */
export const FAKE_COMPUTER: FakeFolder = {
  Users: {
    me: {
      Desktop: {},
      Documents: { 'letter.txt': 1_204, Scans: { 'passport.pdf': 2_400_000 } },
      Downloads: { 'vos-0.1.0.tar.gz': 48_300_000 },
      'todo.md': 310
    },
    Shared: {}
  },
  private: 'private: You do not have permission to open this folder.',
  tmp: {}
}
/** The home directory in `FAKE_COMPUTER`. */
export const FAKE_HOME = '/Users/me'

/** The largest upload the fake host takes: the real host's limit (64 MiB). */
const FAKE_MAX_UPLOAD = 64 * 1024 * 1024

function folderAt(
  root: FakeFolder | undefined,
  path: string
): FakeFolder | number | string | undefined {
  let at: FakeFolder | number | string | undefined = root ?? {}
  for (const name of path.split('/').filter(Boolean)) {
    if (typeof at !== 'object' || !Object.hasOwn(at, name)) return undefined
    at = at[name]
  }
  return at
}

const entryOf = ([name, node]: [string, number | string | FakeFolder]): FileEntry =>
  typeof node === 'number'
    ? { name, kind: 'file', size: node, modifiedAt: FAKE_MODIFIED, link: false }
    : { name, kind: 'dir', size: 0, modifiedAt: FAKE_MODIFIED, link: false }

interface FakeHost extends HostApi {
  /** Each app's Documents, by app id (an app that is not here has none). */
  files: Record<string, FakeFolder>
  /** The host's own file system, from its top; `null` when browsing it is turned off. */
  computer: FakeFolder | null
  /** Takes an upload (`PUT /files/...`) into the fake files, as the host's route does. */
  upload: Uploader
  apps: AppSummary[]
  devices: DeviceRow[]
  /** Push a host notification to the shell (`window.open`, `app.installed`, ...). */
  emit(method: string, params?: Record<string, unknown>): void
}

export function createFakeHost(
  options: {
    apps?: AppSummary[]
    registryErrors?: string[]
    files?: Record<string, FakeFolder>
    /** The host's own file system (not browsable unless given), and the home directory in it. */
    computer?: FakeFolder
    home?: string
    /** Every upload fails with this message. */
    uploadError?: string
    /** An upload stops at this fraction and never answers (the progress state). */
    uploadStallsAt?: number
    /** Every `host.files.*` call fails with this message (a host whose disk cannot be read). */
    filesError?: string
    /** `host.files.list` never answers (the loading state). */
    filesPending?: boolean
  } = {}
): FakeHost {
  const listeners = new Set<(method: string, params: Record<string, unknown>) => void>()
  const fake: FakeHost = {
    apps: [...(options.apps ?? [FAKE_CALCULATOR])],
    devices: fakeDevices(),
    files: structuredClone(options.files ?? {}),
    computer: options.computer ? structuredClone(options.computer) : null,
    async upload(url, file, onProgress) {
      const [path, query] = url.split('?')
      const parts = path.split('/').filter(Boolean).map(decodeURIComponent) // files, host|app, ...
      const root = parts[1] === 'host' ? fake.computer : (fake.files[parts[2]] ??= {})
      const names = parts.slice(parts[1] === 'host' ? 2 : 3)
      const name = names.pop()
      const folder = folderAt(root ?? undefined, names.join('/'))
      if (options.uploadStallsAt !== undefined) {
        onProgress(options.uploadStallsAt)
        return new Promise(() => {})
      }
      if (options.uploadError) return { status: 500, message: options.uploadError }
      if (!root || !name || typeof folder !== 'object')
        return { status: 404, message: `${names.join('/')}: no such file or directory` }
      if (file.size > FAKE_MAX_UPLOAD) return { status: 413, message: 'too large' }
      if (Object.hasOwn(folder, name) && query !== 'replace=true')
        return { status: 409, message: `${name}: something with this name is already here` }
      onProgress(1)
      folder[name] = file.size
      return { status: 201, message: 'saved' }
    },
    emit: (method, params = {}) => listeners.forEach((l) => l(method, params)),
    onNotification(listener) {
      listeners.add(listener)
      return () => void listeners.delete(listener)
    },
    async call(method: string, params: Record<string, unknown> = {}): Promise<unknown> {
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
        case 'app.install': {
          if (fake.apps.some((a) => a.id === NOTES.id))
            throw new HostError(-32004, 'app `notes` is already installed')
          fake.apps.push({
            ...NOTES,
            state: 'running',
            window: { w: 960, h: 640, minW: 480, minH: 360 },
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
        case 'host.files.locations':
          if (options.filesError) throw new HostError(-32003, options.filesError)
          return {
            computer: fake.computer !== null,
            ...(fake.computer !== null && options.home ? { home: options.home } : {}),
            maxUploadBytes: FAKE_MAX_UPLOAD
          }
        case 'host.files.list':
        case 'host.files.remove':
        case 'host.files.createDir': {
          if (options.filesError) throw new HostError(-32003, options.filesError)
          const path = String(params.path ?? '')
          const root = params.app === undefined ? fake.computer : (fake.files[app().id] ??= {})
          if (root === null) throw new HostError(-32003, 'this host does not share its file system')
          if (method === 'host.files.list') {
            if (options.filesPending) return new Promise(() => {})
            const folder = folderAt(root, path)
            if (folder === undefined)
              throw new HostError(-32002, `${path}: no such file or directory`)
            if (typeof folder === 'string')
              throw new HostError(-32003, folder, { kind: 'permission_denied' })
            if (typeof folder !== 'object') throw new HostError(-32003, `${path}: not a directory`)
            const entries = Object.entries(folder).map(entryOf)
            return {
              entries: entries.sort((a, b) => a.name.localeCompare(b.name)),
              truncated: false
            }
          }
          const names = path.split('/').filter(Boolean)
          const name = names.pop()
          const parent = folderAt(root, names.join('/'))
          if (!name || typeof parent !== 'object')
            throw new HostError(-32002, `${path}: no such file or directory`)
          if (method === 'host.files.createDir') {
            if (Object.hasOwn(parent, name))
              throw new HostError(-32004, `${name}: something with this name is already here`)
            parent[name] = {}
            return null
          }
          if (!Object.hasOwn(parent, name))
            throw new HostError(-32002, `${path}: no such file or directory`)
          const doomed = parent[name]
          if (typeof doomed === 'object' && Object.keys(doomed).length > 0 && !params.recursive)
            throw new HostError(-32003, `${name}: The folder is not empty.`, { kind: 'not_empty' })
          delete parent[name]
          return null
        }
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
