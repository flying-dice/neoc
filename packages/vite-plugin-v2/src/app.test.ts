import { expect, test } from 'bun:test'
import { parseApp } from './app.ts'

const pkg = (fields: Record<string, unknown>): string => JSON.stringify({ name: '@vos-apps/notes', version: '1.2.3', ...fields })

test('the id is the package name without its scope, the title defaults to it', () => {
  expect(parseApp(pkg({}))).toEqual({ id: 'notes', title: 'notes', version: '1.2.3' })
})

test('description and the vos field fill in the rest', () => {
  const app = parseApp(
    pkg({ description: 'Notes.', vos: { title: 'Notes', icon: 'note', tint: 'yellow', window: { w: 400, h: 500, minW: 300, minH: 300 } } })
  )
  expect(app).toEqual({ id: 'notes', title: 'Notes', version: '1.2.3', description: 'Notes.', icon: 'note', tint: 'yellow', window: { w: 400, h: 500, minW: 300, minH: 300 } })
})

test('a name that is not an app id, a bad version and an unknown vos key are errors', () => {
  expect(() => parseApp(pkg({ name: '@x/No' }))).toThrow('gives the app id `No`')
  expect(() => parseApp(pkg({ version: '1.0' }))).toThrow('version must be major.minor.patch')
  expect(() => parseApp(pkg({ vos: { colour: 'red' } }))).toThrow('vos.colour is not supported')
})

test('vos.csp and vos.permissions are what the page needs: origins only, and the permissions MCP Apps names', () => {
  const app = parseApp(pkg({ vos: { csp: { resourceDomains: ['https://cdn.example.com', 'https://*.images.example.org'] }, permissions: { camera: {} } } }))
  expect(app.csp).toEqual({ resourceDomains: ['https://cdn.example.com', 'https://*.images.example.org'] })
  expect(app.permissions).toEqual({ camera: {} })
  expect(() => parseApp(pkg({ vos: { csp: { resourceDomains: ['*'] } } }))).toThrow('vos.csp.resourceDomains[0] must be an origin')
  expect(() => parseApp(pkg({ vos: { csp: { scriptDomains: [] } } }))).toThrow('vos.csp.scriptDomains is not supported')
  expect(() => parseApp(pkg({ vos: { permissions: { usb: {} } } }))).toThrow('vos.permissions.usb is not supported')
  expect(() => parseApp(pkg({ vos: { permissions: { camera: true } } }))).toThrow('vos.permissions.camera must be {}')
})
