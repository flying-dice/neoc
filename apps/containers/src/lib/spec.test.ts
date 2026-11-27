import { describe, expect, test } from 'bun:test'
import { blankDraft, blankEntry, blankMount, blankPort, copyName, draftOf, specOf } from './spec'

describe('the spec a form says', () => {
  test('leaves out the rows left empty, and the keys', () => {
    const draft = blankDraft()
    draft.image = 'nginx:1.27'
    draft.environment = [{ ...blankEntry(), name: 'A', value: '1' }, blankEntry()]
    draft.ports = [blankPort(), { ...blankPort(), host_port: '8080', container_port: '80' }]
    draft.mounts = [blankMount('bind'), { ...blankMount('volume'), source: 'data', target: '/data' }]
    const spec = specOf(draft)
    expect(spec.environment).toEqual([{ name: 'A', value: '1' }])
    expect(spec.ports).toEqual([{ host_ip: '', host_port: '8080', container_port: '80', protocol: 'tcp' }])
    expect(spec.mounts).toEqual([{ kind: 'volume', source: 'data', target: '/data', read_only: false }])
  })

  test('filled from a container picks the volumes that are there, and names the others anew', () => {
    const spec = specOf(blankDraft())
    spec.mounts = [
      { kind: 'volume', source: 'data', target: '/data', read_only: false },
      { kind: 'volume', source: 'gone', target: '/gone', read_only: true },
      { kind: 'bind', source: '/srv', target: '/srv', read_only: false }
    ]
    expect(draftOf(spec, ['data']).mounts.map((m) => m.fresh)).toEqual([false, true, false])
  })
})

test('a copy is named after the container, by a name no container has', () => {
  expect(copyName('web', ['web'])).toBe('web-copy')
  expect(copyName('web', ['web', 'web-copy', 'web-copy-2'])).toBe('web-copy-3')
})
