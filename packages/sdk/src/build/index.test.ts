import { afterEach, beforeEach, describe, expect, test } from 'bun:test'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { bundleDescriptorSchema } from '../bundle'
import { buildAppBundle, PublishedVersionError } from './index'

const calculator = resolve(import.meta.dir, '../../../../apps/calculator')
let outDir: string

beforeEach(async () => {
  outDir = await mkdtemp(join(tmpdir(), 'agentos-build-'))
})
afterEach(() => rm(outDir, { recursive: true, force: true }))

describe('buildAppBundle', () => {
  test('writes <id>/bundle.json for the latest and <id>/<version>/ with the files', async () => {
    const built = await buildAppBundle({ appDir: calculator, outDir })
    const { version } = built.manifest
    const latest = bundleDescriptorSchema.parse(await Bun.file(join(outDir, 'calculator/bundle.json')).json())
    expect(latest.server.url).toBe(`${version}/server.js`)
    expect(built.server.url).toBe('server.js')
    for (const file of ['bundle.json', 'server.js', 'client.js']) {
      expect(await Bun.file(join(outDir, 'calculator', version, file)).exists()).toBe(true)
    }
  })

  test('is byte-deterministic: the same source builds to the same sha256s', async () => {
    const first = await buildAppBundle({ appDir: calculator, outDir })
    const second = await buildAppBundle({ appDir: calculator, outDir, immutable: true })
    expect(second).toEqual(first)
  })

  test('immutable accepts identical bytes and refuses a published version whose contents differ', async () => {
    const built = await buildAppBundle({ appDir: calculator, outDir, immutable: true })
    await expect(buildAppBundle({ appDir: calculator, outDir, immutable: true })).resolves.toEqual(built)

    // Pretend version 0.1.0 was published from different bytes.
    const published = Bun.file(join(outDir, 'calculator', built.manifest.version, 'bundle.json'))
    await Bun.write(published, JSON.stringify({ ...built, server: { ...built.server, sha256: 'f'.repeat(64) } }))
    const refused = buildAppBundle({ appDir: calculator, outDir, immutable: true })
    await expect(refused).rejects.toBeInstanceOf(PublishedVersionError)
    await expect(refused).rejects.toThrow(/Bump "version"/)

    // A dev (mutable) build still overwrites it.
    await expect(buildAppBundle({ appDir: calculator, outDir })).resolves.toEqual(built)
  })
})
