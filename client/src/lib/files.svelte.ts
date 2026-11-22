import { HostError } from '../shell/channel'
import { messageOf } from '../shell/errors'
import {
  fileSize,
  fileUrl,
  placeParams,
  type AppSummary,
  type FileEntry,
  type FilePlace,
  type HostApi
} from '../shell/hostApi'
import { callParsed, fileListSchema, fileLocationsSchema } from '../shell/results'
import type { Uploader } from '../shell/upload'

/** An installed app as the Files page lists it: how many things are at the top of its Documents (`null`: not known). */
export interface AppFolder {
  app: AppSummary
  count: number | null
}

/** One file on its way to the host. */
export interface Upload {
  id: number
  /** The name it will have in the folder (another than the file's own after "Keep Both"). */
  name: string
  size: number
  /** The fraction sent, 0 to 1. */
  progress: number
  /**
   * `waiting` behind the one being sent, `uploading`, `done`, `failed` (with `error`), or `conflict`: something with
   * this name is already in the folder, and the person is asked what to do.
   */
  status: 'waiting' | 'uploading' | 'done' | 'failed' | 'conflict'
  error?: string
}

/** What an upload needs besides what is shown of it. */
interface Sending {
  file: Blob
  place: FilePlace
  folder: string[]
}

/** The entry a Remove was asked for, and for a folder how much is in it (`null`: it could not be looked into). */
export interface RemoveRequest {
  entry: FileEntry
  /** How many things are directly in the folder; they go with it, and so does everything in them. */
  inside: number | null
  /** The folder holds more than a listing carries: `inside` is "at least". */
  more: boolean
}

/** JSON-RPC "not found": the folder (or the app) is no longer there. */
const NOT_FOUND = -32002

const byName = (a: string, b: string): number =>
  a.localeCompare(b, undefined, { sensitivity: 'base' })

const samePlace = (a: FilePlace | null, b: FilePlace | null): boolean =>
  a?.kind === b?.kind && (a?.kind !== 'app' || b?.kind !== 'app' || a.app.id === b.app.id)

/** Why `name` cannot be the name of a new folder, or `null`. Mirrors the host's path rules for one name. */
export function folderNameProblem(name: string, taken: string[]): string | null {
  const trimmed = name.trim()
  if (trimmed === '') return null // nothing typed yet: the button is disabled, there is nothing to say
  if (/[/\\:]/.test(trimmed)) return 'A name cannot contain “/”, “\\” or “:”.'
  if (trimmed === '.' || trimmed === '..') return 'A folder needs a name of its own.'
  if (new TextEncoder().encode(trimmed).length > 255) return 'That name is too long.'
  if (taken.some((t) => t.toLowerCase() === trimmed.toLowerCase()))
    return `“${trimmed}” is already here.`
  return null
}

/** `report.csv` → `report (2).csv`, the first such name nothing in `taken` has. */
export function freeName(name: string, taken: string[]): string {
  const dot = name.lastIndexOf('.')
  const [stem, ext] = dot > 0 ? [name.slice(0, dot), name.slice(dot)] : [name, '']
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- a local lookup, gone when the function returns
  const used = new Set(taken.map((t) => t.toLowerCase()))
  for (let n = 2; ; n++) {
    const candidate = `${stem} (${n})${ext}`
    if (!used.has(candidate.toLowerCase())) return candidate
  }
}

/** A path as typed (`/Users/me/Documents`, `~`-less, absolute) as its names from the top; `null` when it is not absolute. */
export function parseHostPath(text: string): string[] | null {
  const trimmed = text.trim()
  if (!trimmed.startsWith('/')) return null
  const names: string[] = []
  for (const name of trimmed.split('/')) {
    if (name === '' || name === '.') continue
    if (name === '..') names.pop()
    else names.push(name)
  }
  return names
}

/**
 * The Files page's state: the places (Home and This Computer when the host lets its own file system be browsed, and
 * the installed apps' Documents), and inside one place its folders and files (`host.files.list`), with a new folder
 * (`host.files.createDir`), removing an entry (`host.files.remove`, after the page has confirmed it) and uploads
 * into the open folder (`PUT /files/...`). One per Files window.
 *
 *   places (place === null)  →  open(place)  →  a folder (path [])  →  enter(name)  →  ...  →  back()
 *
 * An answer that arrives for a folder the person has already left is dropped.
 */
export class FileBrowser {
  /** Whether the host lets its own file system be browsed ("This Computer"). */
  computer = $state(false)
  /** The home directory of the user running the host, as its names from the top; `null` when there is none. */
  home = $state<string[] | null>(null)
  /** The largest file the host takes, in bytes (`null` until the host has said). */
  maxUploadBytes = $state<number | null>(null)
  /** The installed apps: those with something in their Documents first, then by name. */
  folders = $state<AppFolder[]>([])
  /** The place that is open; `null` while the list of places is shown. */
  place = $state<FilePlace | null>(null)
  /** The folder that is open inside the place, as its names from the top. */
  path = $state<string[]>([])
  entries = $state<FileEntry[]>([])
  /** The open folder holds more than a listing carries: `entries` are the first of them. */
  truncated = $state(false)
  /** What is being read: the places, or the open folder. */
  loading = $state(false)
  /** Why the places or the open folder could not be read. */
  error = $state<string | null>(null)
  /** Why the last remove did not happen. The folder under it is still the last one read. */
  removeError = $state<string | null>(null)
  /** What the search field holds: it filters what is on screen (the places, or the open folder) by name. */
  query = $state('')
  /** What a Remove was asked for: the page confirms before anything happens. */
  asked = $state<RemoveRequest | null>(null)
  /** What was typed in the path field and is not a path ("it starts with /"). */
  pathProblem = $state<string | null>(null)
  /** The name being typed for a new folder; `null` when no folder is being made. */
  newFolder = $state<string | null>(null)
  /** What the host said to the last new folder (the name was taken meanwhile, the disk is full). Typing clears it. */
  folderRefusal = $state<string | null>(null)
  /** The files sent from this window, oldest first, until they are dismissed. */
  uploads = $state<Upload[]>([])
  private apps: AppSummary[] = []
  /** Counts every read: an answer is used only if no other read started after it. */
  private reads = 0
  private lastUpload = 0
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- the files on their way: nothing draws from this
  private sending = new Map<number, Sending>()
  private queue: Promise<void> = Promise.resolve()

  constructor(
    private readonly api: HostApi,
    private readonly send: Uploader
  ) {}

  private matches(name: string): boolean {
    return name.toLowerCase().includes(this.query.trim().toLowerCase())
  }

  /** Whether This Computer is among the places the search leaves. */
  get showsComputer(): boolean {
    return this.computer && this.matches('This Computer')
  }

  /** Whether Home (the shortcut to the user's home directory on This Computer) is among them. */
  get showsHome(): boolean {
    return this.computer && this.home !== null && this.matches('Home')
  }

  /** The open folder as a path, on This Computer (`/Users/me`); `''` anywhere else. */
  get pathText(): string {
    return this.place?.kind === 'computer' ? `/${this.path.join('/')}` : ''
  }

  /** The apps the search leaves. */
  get visibleFolders(): AppFolder[] {
    return this.folders.filter((f) => this.matches(f.app.name))
  }

  /** The entries of the open folder the search leaves: folders first, then by name. */
  get visibleEntries(): FileEntry[] {
    return this.entries
      .filter((e) => this.matches(e.name))
      .toSorted(
        (a, b) => Number(b.kind === 'dir') - Number(a.kind === 'dir') || byName(a.name, b.name)
      )
  }

  /** The name of the open place. */
  get placeName(): string {
    if (!this.place) return 'Files'
    return this.place.kind === 'app' ? this.place.app.name : 'This Computer'
  }

  /** The name of what is open: the folder, or the place at its top. */
  get title(): string {
    return this.path.at(-1) ?? this.placeName
  }

  /** Where Back goes: the folder above, the place, or the list of places. */
  get parentTitle(): string {
    if (!this.place) return ''
    if (this.path.length === 0) return 'Files'
    return this.path.at(-2) ?? this.placeName
  }

  /** The first upload that waits for the person: something with its name is already in the folder. */
  get conflict(): Upload | null {
    return this.uploads.find((u) => u.status === 'conflict') ?? null
  }

  /** What is wrong with the new folder's name as typed, or what the host said to it. */
  get folderProblem(): string | null {
    if (this.newFolder === null) return null
    return (
      folderNameProblem(
        this.newFolder,
        this.entries.map((e) => e.name)
      ) ?? this.folderRefusal
    )
  }

  /** Whether the new folder can be made: a name was typed and nothing is wrong with it. */
  get canCreateFolder(): boolean {
    return (
      this.newFolder !== null &&
      this.newFolder.trim() !== '' &&
      folderNameProblem(
        this.newFolder,
        this.entries.map((e) => e.name)
      ) === null
    )
  }

  /** The installed apps changed (or the page opened): read the places again, and leave an app that is gone. */
  async setApps(apps: AppSummary[]): Promise<void> {
    this.apps = apps
    const open = this.place
    if (open?.kind === 'app' && !apps.some((a) => a.id === open.app.id)) this.close()
    if (!this.place) await this.loadPlaces()
  }

  private async loadPlaces(): Promise<void> {
    const read = ++this.reads
    this.loading = true
    this.error = null
    const locations = callParsed(this.api, 'host.files.locations', {}, fileLocationsSchema).catch(
      (e: unknown) => messageOf(e)
    )
    const counted = await Promise.all(
      this.apps.map(async (app): Promise<AppFolder & { failed?: string }> => {
        try {
          const { entries } = await this.list({ kind: 'app', app }, [])
          return { app, count: entries.length }
        } catch (e) {
          return { app, count: null, failed: messageOf(e) }
        }
      })
    )
    const shared = await locations
    if (read !== this.reads) return
    this.folders = counted
      .map(({ app, count }) => ({ app, count }))
      .toSorted((a, b) => Number(!!b.count) - Number(!!a.count) || byName(a.app.name, b.app.name))
    // one app that cannot be read is said on its row; when nothing can, the host is the problem
    const failed = counted.filter((f) => f.failed)
    if (typeof shared === 'string') {
      this.error = shared
    } else {
      this.computer = shared.computer
      this.home = shared.home === undefined ? null : parseHostPath(shared.home)
      this.maxUploadBytes = shared.maxUploadBytes
      this.error = failed.length > 0 && failed.length === counted.length ? failed[0].failed! : null
    }
    this.loading = false
  }

  /** The params that name a folder or entry of a place: on This Computer its absolute path, in an app a relative one. */
  private at(place: FilePlace, path: string[]): { app?: string; path: string } {
    const joined = path.join('/')
    return { ...placeParams(place), path: place.kind === 'computer' ? `/${joined}` : joined }
  }

  private list(
    place: FilePlace,
    path: string[]
  ): Promise<{ entries: FileEntry[]; truncated: boolean }> {
    return callParsed(this.api, 'host.files.list', this.at(place, path), fileListSchema)
  }

  /** Open a place: at its top, or on This Computer at `path` (Home is This Computer at the home directory). */
  open(place: FilePlace, path: string[] = []): Promise<void> {
    this.place = place
    return this.go(path)
  }

  /** The path field: go to the folder typed or pasted, anywhere on This Computer. A path that is not absolute is said, not sent. */
  goTo(text: string): Promise<void> {
    const names = parseHostPath(text)
    if (names === null) {
      this.pathProblem = 'A path on this computer starts with “/”, like /Users or /tmp.'
      return Promise.resolve()
    }
    this.pathProblem = null
    this.place = { kind: 'computer' }
    return this.go(names)
  }

  /** Open a folder of the open folder. */
  enter(name: string): Promise<void> {
    return this.go([...this.path, name])
  }

  /** Up one level: the folder above, or from the top of a place the list of places. */
  back(): Promise<void> {
    if (!this.place) return Promise.resolve()
    if (this.path.length > 0) return this.go(this.path.slice(0, -1))
    this.close()
    return this.loadPlaces()
  }

  /** Read what is on screen again. */
  refresh(): Promise<void> {
    return this.place ? this.go(this.path) : this.loadPlaces()
  }

  private close(): void {
    this.place = null
    this.path = []
    this.entries = []
    this.truncated = false
    this.leaveFolder()
  }

  /** What belongs to the folder that was on screen, and not to the next one. */
  private leaveFolder(): void {
    this.query = ''
    this.asked = null
    this.removeError = null
    this.pathProblem = null
    this.newFolder = null
    this.folderRefusal = null
  }

  private isOpen(place: FilePlace, folder: string[]): boolean {
    return samePlace(this.place, place) && this.path.join('/') === folder.join('/')
  }

  private async go(path: string[]): Promise<void> {
    const place = this.place
    if (!place) return
    const read = ++this.reads
    const moved = path.join('/') !== this.path.join('/') || this.error !== null
    this.path = path
    this.loading = true
    this.error = null
    if (moved) {
      // another folder: what was on screen is not in it, and neither is what was searched for
      this.entries = []
      this.truncated = false
      this.leaveFolder()
    }
    try {
      const { entries, truncated } = await this.list(place, path)
      if (read !== this.reads) return
      this.entries = entries
      this.truncated = truncated
    } catch (e) {
      if (read !== this.reads) return
      this.entries = []
      this.truncated = false
      this.error = messageOf(e)
    }
    this.loading = false
  }

  /** The path of an entry of the open folder, from the top of the place. */
  private pathOf(name: string): string[] {
    return [...this.path, name]
  }

  /** Where a file of the open folder downloads from. */
  downloadUrl(entry: FileEntry): string {
    return this.place ? fileUrl(this.place, this.pathOf(entry.name)) : ''
  }

  /**
   * A Remove was asked for an entry of the open folder: ready the confirmation. For a folder that means looking into
   * it first, so the question can say how much goes with it. (A link is only ever removed as the link.)
   */
  async ask(entry: FileEntry): Promise<void> {
    const place = this.place
    if (!place) return
    let request: RemoveRequest = { entry, inside: null, more: false }
    if (entry.kind === 'dir' && !entry.link) {
      try {
        const { entries, truncated } = await this.list(place, this.pathOf(entry.name))
        request = { entry, inside: entries.length, more: truncated }
      } catch {
        // it cannot be looked into (no permission, say): the question says so, and the remove will say why
      }
    }
    if (this.place === place) this.asked = request
  }

  /**
   * Remove an entry of the open folder, then read the folder again. A folder goes with everything in it, which is
   * what the confirmation said; a file or a link goes alone.
   */
  async remove(entry: FileEntry): Promise<void> {
    const place = this.place
    if (!place) return
    this.asked = null
    this.removeError = null
    const recursive = entry.kind === 'dir' && !entry.link
    try {
      const target = this.at(place, this.pathOf(entry.name))
      await this.api.call('host.files.remove', recursive ? { ...target, recursive } : target)
    } catch (e) {
      // already gone (the app removed it first) is what was asked for
      if (!(e instanceof HostError && e.code === NOT_FOUND)) this.removeError = messageOf(e)
    }
    if (this.place === place) await this.go(this.path)
  }

  // ── A new folder ────────────────────────────────────────────────────

  /** Start naming a new folder in the open folder. */
  startFolder(): void {
    if (this.place) this.newFolder = ''
  }

  /** The name being typed changed: what the host said to the last one no longer applies. */
  nameFolder(name: string): void {
    this.newFolder = name
    this.folderRefusal = null
  }

  cancelFolder(): void {
    this.newFolder = null
    this.folderRefusal = null
  }

  /** Make the folder that was named, then read the open folder again. A name with a problem is not sent. */
  async createFolder(): Promise<void> {
    const place = this.place
    if (!place || this.newFolder === null || !this.canCreateFolder) return
    const name = this.newFolder.trim()
    try {
      await this.api.call('host.files.createDir', this.at(place, this.pathOf(name)))
    } catch (e) {
      this.folderRefusal = messageOf(e)
      return
    }
    this.newFolder = null
    if (this.place === place) await this.go(this.path)
  }

  // ── Uploads ─────────────────────────────────────────────────────────

  /**
   * Send files into the open folder, one after another. A file over the host's limit is not sent. One whose name is
   * taken waits as a `conflict` for `resolveConflict`; the others go on.
   */
  upload(files: { name: string; size: number; blob: Blob }[]): Promise<void> {
    const place = this.place
    if (!place) return Promise.resolve()
    const folder = [...this.path]
    for (const file of files) {
      const id = ++this.lastUpload
      const tooBig = this.maxUploadBytes !== null && file.size > this.maxUploadBytes
      this.uploads.push({
        id,
        name: file.name,
        size: file.size,
        progress: 0,
        status: tooBig ? 'failed' : 'waiting',
        error: tooBig
          ? `Larger than ${fileSize(this.maxUploadBytes ?? 0)}, the most this host takes.`
          : undefined
      })
      if (tooBig) continue
      this.sending.set(id, { file: file.blob, place, folder })
      this.enqueue(id, false)
    }
    return this.queue
  }

  private enqueue(id: number, replace: boolean): void {
    this.queue = this.queue.then(() => this.sendOne(id, replace))
  }

  private change(id: number, patch: Partial<Upload>): void {
    const upload = this.uploads.find((u) => u.id === id)
    if (upload) Object.assign(upload, patch)
  }

  private async sendOne(id: number, replace: boolean): Promise<void> {
    const upload = this.uploads.find((u) => u.id === id)
    const sending = this.sending.get(id)
    if (!upload || !sending) return // dismissed while it waited
    // a name the folder on screen already has: ask now, without sending the file to be told so (the host refuses it
    // all the same if the folder changed since it was read)
    const here = this.isOpen(sending.place, sending.folder) && !this.loading
    if (!replace && here && this.entries.some((e) => e.name === upload.name)) {
      this.change(id, { status: 'conflict' })
      return
    }
    this.change(id, { status: 'uploading', progress: 0, error: undefined })
    const url = fileUrl(sending.place, [...sending.folder, upload.name])
    const answer = await this.send(
      replace ? `${url}?replace=true` : url,
      sending.file,
      (fraction) => this.change(id, { progress: Math.min(1, Math.max(0, fraction)) })
    )
    if (answer.status === 201) {
      this.sending.delete(id)
      this.change(id, { status: 'done', progress: 1 })
      // the new file is in the list, if the person is still looking at that folder
      if (this.isOpen(sending.place, sending.folder)) await this.go(this.path)
    } else if (answer.status === 409) {
      this.change(id, { status: 'conflict' })
    } else {
      this.sending.delete(id)
      const error =
        answer.status === 413
          ? 'Too large for this host.'
          : answer.message.trim() || `The host answered ${answer.status}.`
      this.change(id, { status: 'failed', error })
    }
  }

  /**
   * What to do with an upload whose name is taken: `replace` the file that is there, `keep` both (the new one gets
   * the first free name, `report (2).csv`), or `cancel` this upload.
   */
  resolveConflict(id: number, choice: 'replace' | 'keep' | 'cancel'): Promise<void> {
    const upload = this.uploads.find((u) => u.id === id && u.status === 'conflict')
    const sending = this.sending.get(id)
    if (!upload || !sending) return Promise.resolve()
    if (choice === 'cancel') {
      this.sending.delete(id)
      this.uploads = this.uploads.filter((u) => u.id !== id)
      return Promise.resolve()
    }
    if (choice === 'keep') {
      // the names in the folder as last read, when it is the one on screen, and the names already on their way
      const here = this.isOpen(sending.place, sending.folder) ? this.entries.map((e) => e.name) : []
      const taken = [...here, ...this.uploads.map((u) => u.name)]
      upload.name = freeName(upload.name, taken)
    }
    upload.status = 'waiting'
    this.enqueue(id, choice === 'replace')
    return this.queue
  }

  /** Forget the uploads that are over (done or failed). */
  dismissUploads(): void {
    this.uploads = this.uploads.filter((u) => u.status !== 'done' && u.status !== 'failed')
  }
}
