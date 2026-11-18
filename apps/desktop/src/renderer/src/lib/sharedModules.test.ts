import { expect, test } from 'bun:test'
import { SHARED_MODULES } from '@agentos/sdk/build'

test('the shell provides exactly the modules app bundles expect to share', async () => {
  const source = await Bun.file(new URL('./sharedModules.ts', import.meta.url)).text()
  const start = source.indexOf('Object.freeze({')
  const table = source.slice(start, source.indexOf('})', start))
  // `'svelte/store': svelteStore,` or the shorthand `svelte,`
  const keys = [...table.matchAll(/^\s+(?:'([^']+)':|([A-Za-z]+),)/gm)].map((m) => m[1] ?? m[2])
  expect(keys.sort()).toEqual([...SHARED_MODULES].sort())
})
