type Entry = VosApp.Containers.Entry
type Mount = VosApp.Containers.Mount
type MountKind = VosApp.Containers.MountKind
type Port = VosApp.Containers.Port
type Spec = VosApp.Containers.Spec

/**
 * A container's spec as the form holds it while it is filled in: each row of a list has a `key` that keeps it apart
 * from the others while rows come and go, and a volume says whether it is one that is there (picked) or one the form
 * names (`fresh`). What it means is the backend's to check: nothing here says a port is wrong.
 */
export type Keyed<T> = T & { key: number }
/** A mount as the form holds it. `fresh`: a volume the form names, which Docker makes; else one picked from those there. */
export type DraftMount = Keyed<Mount & { fresh: boolean }>
export interface Draft extends Omit<Spec, 'environment' | 'labels' | 'ports' | 'mounts'> {
  environment: Keyed<Entry>[]
  labels: Keyed<Entry>[]
  ports: Keyed<Port>[]
  mounts: DraftMount[]
}

let keys = 0
const key = (): number => ++keys

export const blankEntry = (): Keyed<Entry> => ({ key: key(), name: '', value: '' })
export const blankPort = (): Keyed<Port> => ({ key: key(), host_ip: '', host_port: '', container_port: '', protocol: 'tcp' })
export const blankMount = (kind: MountKind = 'bind'): DraftMount => ({ key: key(), kind, source: '', target: '', read_only: false, fresh: true })

/** The form for a new container: nothing but Docker's defaults, started once it is made. */
export function blankDraft(): Draft {
  return {
    image: '',
    name: '',
    command: '',
    entrypoint: '',
    environment: [],
    ports: [],
    publish_all: false,
    mounts: [],
    network: 'bridge',
    restart: 'no',
    retries: '',
    labels: [],
    working_dir: '',
    user: '',
    hostname: '',
    start: true
  }
}

/** The form filled from a container's spec; `volumes` are the names of the volumes there are, which it picks from. */
export function draftOf(spec: Spec, volumes: readonly string[]): Draft {
  return {
    ...spec,
    environment: spec.environment.map((e) => ({ ...e, key: key() })),
    labels: spec.labels.map((e) => ({ ...e, key: key() })),
    ports: spec.ports.map((p) => ({ ...p, key: key() })),
    mounts: spec.mounts.map((m) => ({ ...m, key: key(), fresh: m.kind === 'volume' && !volumes.includes(m.source) }))
  }
}

/** The spec the form says, as the backend takes it: its rows without their keys, and a row left empty left out. */
export function specOf(draft: Draft): Spec {
  const entry = ({ name, value }: Entry): Entry => ({ name, value })
  const filled = (e: Entry): boolean => e.name.trim() !== '' || e.value !== ''
  return {
    ...draft,
    environment: draft.environment.filter(filled).map(entry),
    labels: draft.labels.filter(filled).map(entry),
    ports: draft.ports
      .filter((p) => `${p.host_ip}${p.host_port}${p.container_port}`.trim() !== '')
      .map(({ host_ip, host_port, container_port, protocol }) => ({ host_ip, host_port, container_port, protocol })),
    mounts: draft.mounts
      .filter((m) => `${m.source}${m.target}`.trim() !== '')
      .map(({ kind, source, target, read_only }) => ({ kind, source, target, read_only }))
  }
}

/** A name for a copy of a container that no container has: `web-copy`, then `web-copy-2`, ... */
export function copyName(name: string, taken: readonly string[]): string {
  const first = `${name}-copy`
  if (!taken.includes(first)) return first
  for (let n = 2; ; n++) if (!taken.includes(`${first}-${n}`)) return `${first}-${n}`
}
