/**
 * What the app is, from its `package.json`: the package is the app.
 *
 *     {
 *       "name": "@vos-apps/app1",           // the app's id: the name without its scope
 *       "version": "0.1.0",                 // the app's version
 *       "description": "...",               // what the shell shows about it
 *       "vos": { "title": "App 1", "icon": "square.and.pencil", "tint": "orange", "window": { "w": 360, "h": 420, "minW": 320, "minH": 320 } }
 *     }
 *
 * `vos.title` is the name a person sees (the id when absent). Its API is the contract's (`openrpc.json`, `contract.ts`).
 * `vos.csp` and `vos.permissions` are what its page needs, in the shape of MCP Apps' `_meta.ui`: the host builds the page's
 * content security policy from `csp` (nothing external when it is absent: `resourceDomains` are origins images, media,
 * fonts and styles may come from; the other lists are parsed, not applied) and the frame's `allow` from `permissions`.
 */

export interface AppMeta {
  /** 3 to 40 characters: lowercase letters, digits, dashes; namespaces the app's data and tools. */
  id: string
  title: string
  version: string
  description?: string
  icon?: string
  tint?: string
  window?: { w: number; h: number; minW: number; minH: number }
  csp?: Csp
  permissions?: Partial<Record<(typeof PERMISSIONS)[number], Record<string, never>>>
}

export interface Csp {
  connectDomains?: string[]
  resourceDomains?: string[]
  frameDomains?: string[]
  baseUriDomains?: string[]
}

/** The permissions MCP Apps names. */
export const PERMISSIONS = ['camera', 'microphone', 'geolocation', 'clipboardWrite'] as const

/** An origin an app declares: `https://host[:port]`, or `https://*.domain.tld` for the subdomains of one domain. Never a bare scheme or `*`. */
const ORIGIN = /^https?:\/\/(\*\.)?(([a-z0-9]([a-z0-9-]*[a-z0-9])?)\.)+[a-z]{2,}(:\d{1,5})?$|^https?:\/\/(localhost|\d{1,3}(\.\d{1,3}){3})(:\d{1,5})?$/i
const KEYS = ['title', 'icon', 'tint', 'window', 'csp', 'permissions']

export class AppError extends Error {
  override name = 'AppError'
}

type Json = null | boolean | number | string | Json[] | { [key: string]: Json }
type JsonObject = { [key: string]: Json }

const isObject = (v: Json | undefined): v is JsonObject => typeof v === 'object' && v !== null && !Array.isArray(v)

export function parseApp(source: string, file = 'package.json'): AppMeta {
  const fail = (path: string, message: string): never => {
    throw new AppError(`${file}: ${path} ${message}`)
  }
  let pkg: Json
  try {
    pkg = JSON.parse(source) as Json
  } catch (e) {
    return fail('$', `is not JSON (${(e as Error).message})`)
  }
  if (!isObject(pkg)) return fail('$', 'must be an object')
  if (typeof pkg.name !== 'string') return fail('name', 'is required: the app id is the package name without its scope')
  const id = pkg.name.replace(/^@[^/]+\//, '')
  if (!/^[a-z0-9][a-z0-9-]{2,39}$/.test(id)) fail('name', `gives the app id \`${id}\`: it must be 3 to 40 lowercase letters, digits and dashes`)
  if (id === 'vos') fail('name', "gives the app id `vos`: that is the app framework's own scope (`@vos/*`): pick another")
  if (typeof pkg.version !== 'string' || !/^\d+\.\d+\.\d+$/.test(pkg.version)) fail('version', 'must be major.minor.patch')
  const meta: AppMeta = { id, title: id, version: pkg.version as string }
  if (pkg.description !== undefined) {
    if (typeof pkg.description !== 'string' || pkg.description.length > 500) fail('description', 'must be text of at most 500 characters')
    meta.description = pkg.description as string
  }
  const vos = pkg.vos
  if (vos === undefined) return meta
  if (!isObject(vos)) return fail('vos', `must be an object (${KEYS.join(', ')})`)
  for (const key of Object.keys(vos)) if (!KEYS.includes(key)) fail(`vos.${key}`, `is not supported (${KEYS.join(', ')})`)
  if (vos.title !== undefined) {
    if (typeof vos.title !== 'string' || vos.title.trim() === '' || vos.title.length > 60) fail('vos.title', 'must be the name a person sees, at most 60 characters')
    meta.title = vos.title as string
  }
  if (vos.icon !== undefined) {
    if (typeof vos.icon !== 'string' || !/^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/.test(vos.icon)) fail('vos.icon', 'must be a symbol name: letters, digits, `.`, `_` and `-`')
    meta.icon = vos.icon as string
  }
  if (vos.tint !== undefined) {
    if (typeof vos.tint !== 'string' || !/^[a-z][a-z0-9-]{0,31}$/.test(vos.tint)) fail('vos.tint', 'must be a palette role such as `blue` or `orange`')
    meta.tint = vos.tint as string
  }
  if (vos.window !== undefined) {
    const w = vos.window
    if (!isObject(w)) return fail('vos.window', 'must be { w, h, minW, minH }')
    for (const key of Object.keys(w)) if (!['w', 'h', 'minW', 'minH'].includes(key)) fail(`vos.window.${key}`, 'is not supported (w, h, minW, minH)')
    const size = (key: 'w' | 'h' | 'minW' | 'minH'): number => {
      const n = w[key]
      if (typeof n !== 'number' || !Number.isInteger(n) || n <= 0) fail(`vos.window.${key}`, 'must be a whole number of points')
      return n as number
    }
    meta.window = { w: size('w'), h: size('h'), minW: size('minW'), minH: size('minH') }
  }
  if (vos.csp !== undefined) {
    const given = vos.csp
    if (!isObject(given)) return fail('vos.csp', 'must be { resourceDomains, connectDomains, frameDomains, baseUriDomains }')
    const keys = ['connectDomains', 'resourceDomains', 'frameDomains', 'baseUriDomains'] as const
    for (const key of Object.keys(given)) if (!(keys as readonly string[]).includes(key)) fail(`vos.csp.${key}`, `is not supported (${keys.join(', ')})`)
    const csp: Csp = {}
    for (const key of keys) {
      const list = given[key]
      if (list === undefined) continue
      if (!Array.isArray(list)) return fail(`vos.csp.${key}`, 'must be a list of origins')
      csp[key] = list.map((origin, i) => {
        if (typeof origin !== 'string' || !ORIGIN.test(origin))
          fail(`vos.csp.${key}[${i}]`, 'must be an origin such as https://cdn.example.com or https://*.example.com (no bare scheme, no `*`)')
        return origin as string
      })
    }
    meta.csp = csp
  }
  if (vos.permissions !== undefined) {
    const given = vos.permissions
    if (!isObject(given)) return fail('vos.permissions', 'must be an object such as { "camera": {} }')
    for (const key of Object.keys(given)) if (!(PERMISSIONS as readonly string[]).includes(key)) fail(`vos.permissions.${key}`, `is not supported (${PERMISSIONS.join(', ')})`)
    const permissions: NonNullable<AppMeta['permissions']> = {}
    for (const name of PERMISSIONS) {
      const one = given[name]
      if (one === undefined) continue
      if (!isObject(one) || Object.keys(one).length > 0) fail(`vos.permissions.${name}`, 'must be {}')
      permissions[name] = {}
    }
    meta.permissions = permissions
  }
  return meta
}
