type Entry = VosApp.Containers.Entry
type Image = VosApp.Containers.Image
type ImagesPruned = VosApp.Containers.ImagesPruned
type Network = VosApp.Containers.Network
type Pulling = VosApp.Containers.PullingParams
type Volume = VosApp.Containers.Volume
type VolumesPruned = VosApp.Containers.VolumesPruned
import { bytes, count, day, imageName, listed, shortId } from './format'
import type { Keyed } from './spec'

/**
 * What the window needs of the backend for Docker's images, volumes and networks: each list, and each way of changing
 * it (the app's private methods on the bus, `_images` ...: `App.svelte` sends them). Every change answers with its list
 * as it now stands.
 */
export interface ResourcesRpc {
  images(): Promise<Image[]>
  pullImage(params: VosApp.Containers.ImagePullParams): Promise<Image[]>
  removeImage(params: VosApp.Containers.ImageRemoveParams): Promise<Image[]>
  pruneImages(): Promise<ImagesPruned>
  volumes(): Promise<Volume[]>
  createVolume(params: VosApp.Containers.VolumeCreateParams): Promise<Volume[]>
  removeVolume(params: VosApp.Containers.VolumeRemoveParams): Promise<Volume[]>
  pruneVolumes(): Promise<VolumesPruned>
  networks(): Promise<Network[]>
  createNetwork(params: VosApp.Containers.NetworkCreateParams): Promise<Network[]>
  removeNetwork(params: VosApp.Containers.NetworkRemoveParams): Promise<Network[]>
}

/** The lists there are: Docker's images, volumes and networks. */
export type Kind = 'images' | 'volumes' | 'networks'

/** The pull of an image by its name, from the list of images. */
export interface Pull {
  /** The name typed: `nginx:1.27`. */
  reference: string
  busy: boolean
  /** How it goes, once Docker has said. */
  progress: Pulling | null
  /** Why it could not be pulled, in the backend's words. */
  failure: string | null
}

/** A volume as the form holds it while it is filled in. */
export interface VolumeDraft {
  name: string
  driver: string
  labels: Keyed<Entry>[]
}
/** A network as the form holds it while it is filled in. */
export interface NetworkDraft {
  name: string
  driver: string
  subnet: string
  gateway: string
  internal: boolean
  attachable: boolean
  labels: Keyed<Entry>[]
}
/** The form a volume or a network is made in, while it is open. */
export interface ObjectForm<D> {
  draft: D
  /** Why the backend refused it, in its words. */
  failure: string | null
  saving: boolean
}

/** The question before an image, a volume or a network is removed. */
export interface Dropping {
  kind: 'image' | 'volume' | 'network'
  /** What the backend is given: an image's id, a volume's or a network's name. */
  key: string
  /** What a person calls it. */
  name: string
  /** A line about it, under its name. */
  about: string
  /** The containers that use it, by name. */
  users: readonly string[]
  /** An image: remove it though containers were made from it. */
  force: boolean
  /** Why it was refused, in the backend's words. */
  failure: string | null
  busy: boolean
}

const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))

/** A row left empty is left out, and a row loses its key: the labels as the backend takes them. */
const labelsOf = (rows: readonly Keyed<Entry>[]): Entry[] =>
  rows.filter((e) => e.name.trim() !== '' || e.value !== '').map(({ name, value }) => ({ name, value }))

export const blankVolume = (): VolumeDraft => ({ name: '', driver: 'local', labels: [] })
export const blankNetwork = (): NetworkDraft => ({ name: '', driver: 'bridge', subnet: '', gateway: '', internal: false, attachable: false, labels: [] })

/** The images a prune of the dangling ones takes: those with no name that no container was made from. */
export const danglingOf = (images: readonly Image[]): Image[] => images.filter((i) => i.tags.length === 0 && i.containers.length === 0)
/** The volumes a prune takes: those no container uses. */
export const unusedOf = (volumes: readonly Volume[]): Volume[] => volumes.filter((v) => v.containers.length === 0)

/**
 * One window's images, volumes and networks: Docker's, read again whenever Docker changes, and what is the window's
 * own while they are changed (the pull, a form, the question before a removal, what the last prune freed). A failure
 * that belongs to no form or question is said where the window says the rest (`fail`).
 */
export class Resources {
  #images = $state.raw<readonly Image[]>([])
  #volumes = $state.raw<readonly Volume[]>([])
  #networks = $state.raw<readonly Network[]>([])
  /** The pull from the list of images. */
  pull = $state<Pull>({ reference: '', busy: false, progress: null, failure: null })
  /** The form a volume is made in, while it is open. */
  volumeForm = $state<ObjectForm<VolumeDraft> | null>(null)
  /** The form a network is made in, while it is open. */
  networkForm = $state<ObjectForm<NetworkDraft> | null>(null)
  /** The question before something is removed, while it is asked. */
  removal = $state<Dropping | null>(null)
  /** What the last prune took, in words, for the list it was made from. */
  report = $state<{ kind: Kind; text: string } | null>(null)
  #pruning = $state<Kind | null>(null)

  readonly #rpc: ResourcesRpc
  readonly #fail: (message: string) => void

  constructor(rpc: ResourcesRpc, fail: (message: string) => void) {
    this.#rpc = rpc
    this.#fail = fail
  }

  get images(): readonly Image[] {
    return this.#images
  }
  get volumes(): readonly Volume[] {
    return this.#volumes
  }
  get networks(): readonly Network[] {
    return this.#networks
  }
  /** A prune of this list is in flight. */
  pruning(kind: Kind): boolean {
    return this.#pruning === kind
  }
  /** The form that is open can be saved: it is not being saved, and a network's names it. */
  get savable(): boolean {
    if (this.volumeForm) return !this.volumeForm.saving
    return this.networkForm !== null && !this.networkForm.saving && this.networkForm.draft.name.trim() !== ''
  }

  /** Read the lists again (all of them, or those asked for). A failure is said; what was there stays. */
  async load(kinds: readonly Kind[] = ['images', 'volumes', 'networks']): Promise<void> {
    await Promise.all(
      kinds.map(async (kind) => {
        try {
          if (kind === 'images') this.#images = await this.#rpc.images()
          else if (kind === 'volumes') this.#volumes = await this.#rpc.volumes()
          else this.#networks = await this.#rpc.networks()
        } catch (e) {
          this.#fail(messageOf(e))
        }
      })
    )
  }

  /** How the pull of an image goes: the list's own pull, when it is that image's. */
  hear(progress: Pulling): void {
    if (this.pull.busy && progress.image === this.pull.reference.trim()) this.pull.progress = progress
  }

  /** Pull the image whose name is typed. Its name is kept, with the backend's reason, when it cannot be pulled. */
  async pullImage(): Promise<void> {
    const pull = this.pull
    if (pull.busy || pull.reference.trim() === '') return
    pull.busy = true
    pull.failure = null
    pull.progress = null
    try {
      this.#images = await this.#rpc.pullImage({ image: pull.reference.trim() })
      pull.reference = ''
    } catch (e) {
      pull.failure = messageOf(e)
    } finally {
      pull.busy = false
      pull.progress = null
    }
  }

  /** Leave whatever is open (a form, a question) and what the last prune said. */
  close(): void {
    if (this.volumeForm?.saving || this.networkForm?.saving || this.removal?.busy) return
    this.volumeForm = null
    this.networkForm = null
    this.removal = null
    this.report = null
  }

  /** Ask before an image is removed. */
  askImage(image: Image): void {
    this.close()
    this.removal = {
      kind: 'image',
      key: image.id,
      name: imageName(image),
      about: `${shortId(image.id)} · ${bytes(image.size)} · built ${day(new Date(image.created * 1000))}`,
      users: image.containers,
      force: false,
      failure: null,
      busy: false
    }
  }

  /** Ask before a volume is removed, with its data. */
  askVolume(volume: Volume): void {
    this.close()
    this.removal = { kind: 'volume', key: volume.name, name: volume.name, about: `${volume.driver} · ${volume.mountpoint}`, users: volume.containers, force: false, failure: null, busy: false }
  }

  /** Ask before a network is removed. Docker's own are not: they cannot be removed. */
  askNetwork(network: Network): void {
    if (network.predefined) return
    this.close()
    const addresses = network.subnet === '' ? 'no subnet' : network.subnet
    this.removal = { kind: 'network', key: network.name, name: network.name, about: `${network.driver} · ${addresses}`, users: network.containers, force: false, failure: null, busy: false }
  }

  /** Remove what was asked about. The question stays, with the reason, when it is refused. */
  async remove(): Promise<void> {
    const removal = this.removal
    if (removal === null || removal.busy) return
    removal.busy = true
    removal.failure = null
    try {
      if (removal.kind === 'image') this.#images = await this.#rpc.removeImage({ image: removal.key, force: removal.force })
      else if (removal.kind === 'volume') this.#volumes = await this.#rpc.removeVolume({ volume: removal.key })
      else this.#networks = await this.#rpc.removeNetwork({ network: removal.key })
      removal.busy = false
      this.removal = null
    } catch (e) {
      removal.failure = messageOf(e)
      removal.busy = false
    }
  }

  /** Remove the dangling images, or the unused volumes with their data, and say what that took. */
  async prune(kind: 'images' | 'volumes'): Promise<void> {
    if (this.#pruning !== null) return
    this.#pruning = kind
    this.report = null
    try {
      if (kind === 'images') {
        const pruned = await this.#rpc.pruneImages()
        this.#images = pruned.images
        this.report = {
          kind,
          text: pruned.removed === 0 ? 'There were no dangling images.' : `Removed ${count(pruned.removed, 'dangling image')}, and freed ${bytes(pruned.space)}.`
        }
      } else {
        const pruned = await this.#rpc.pruneVolumes()
        this.#volumes = pruned.volumes
        this.report = {
          kind,
          text:
            pruned.removed.length === 0
              ? 'There were no unused volumes.'
              : `Deleted ${listed(pruned.removed)} with ${pruned.removed.length === 1 ? 'its' : 'their'} data, and freed ${bytes(pruned.space)}.`
        }
      }
    } catch (e) {
      this.#fail(messageOf(e))
    } finally {
      this.#pruning = null
    }
  }

  /** Open the form for a new volume. */
  composeVolume(): void {
    this.close()
    this.volumeForm = { draft: blankVolume(), failure: null, saving: false }
  }

  /** Open the form for a new network. */
  composeNetwork(): void {
    this.close()
    this.networkForm = { draft: blankNetwork(), failure: null, saving: false }
  }

  /** Save the form that is open. It stays open, with the backend's reason, when it is refused. */
  async save(): Promise<void> {
    if (!this.savable) return
    const form = this.volumeForm ?? this.networkForm
    if (form === null) return
    form.saving = true
    form.failure = null
    try {
      const volume = this.volumeForm
      if (volume) {
        const { name, driver, labels } = volume.draft
        this.#volumes = await this.#rpc.createVolume({ name: name.trim(), driver: driver.trim(), labels: labelsOf(labels) })
        this.volumeForm = null
      } else if (this.networkForm) {
        const { labels, ...rest } = this.networkForm.draft
        this.#networks = await this.#rpc.createNetwork({ ...rest, labels: labelsOf(labels) })
        this.networkForm = null
      }
    } catch (e) {
      form.failure = messageOf(e)
    } finally {
      form.saving = false
    }
  }

  /** Leave the form that is open. */
  cancel(): void {
    if (this.volumeForm?.saving || this.networkForm?.saving) return
    this.volumeForm = null
    this.networkForm = null
  }
}
