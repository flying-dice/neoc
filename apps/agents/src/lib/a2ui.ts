/**
 * A2UI as it rides in AG-UI: an activity of type `a2ui-surface` whose content holds a list of operations
 * (`a2ui_operations`), each creating a surface, giving it components, changing its data, or deleting it. This reads
 * the operations into surfaces and resolves the values of their components against the surface's data.
 *
 * Both wire forms are read: A2UI 0.9 (`{ "path": … }`, `{ "call": … }`) and 1.0 (`{ "@path": … }`, `{ "@call": … }`,
 * components and data inside `createSurface`). What comes from the agent is data: a shape this does not know is
 * left out, never an error.
 */

export const A2UI_ACTIVITY = 'a2ui-surface'

export type Json = null | boolean | number | string | Json[] | { [key: string]: Json }
type Record_ = { [key: string]: Json }

/** A component as the agent wrote it: an id, the name of its kind, and its properties beside them. */
export interface Component {
  id: string
  component: string
  [property: string]: Json
}

export interface Surface {
  id: string
  components: Map<string, Component>
  data: Json
}

/** Where a component's relative paths start: the item of the list it was repeated for. */
export type Scope = string

const isRecord = (value: unknown): value is Record_ => typeof value === 'object' && value !== null && !Array.isArray(value)

/** The most components and surfaces an activity is read to: an agent cannot make the window draw without end. */
const MAX_COMPONENTS = 2000
const MAX_SURFACES = 16
/** The most operations of an activity that are read, and the deepest a path of the data may go. */
const MAX_OPERATIONS = 2000
const MAX_PATH = 32
/** The most children of one container that are drawn, the deepest a component is drawn, and the most a surface draws. */
const MAX_CHILDREN = 200
export const MAX_DEPTH = 24
const MAX_DRAWN = 4000

/** The steps of a JSON Pointer (`/a/0/b`). */
function steps(pointer: string): string[] {
  return pointer
    .split('/')
    .filter((step, i) => i > 0 || step !== '')
    .filter((step) => step !== '')
    .map((step) => step.replaceAll('~1', '/').replaceAll('~0', '~'))
}

/** A path of a component, as a pointer from the root of the data: a relative one starts at the scope. */
export function absolute(path: string, scope: Scope): string {
  return path.startsWith('/') ? path : `${scope.replace(/\/$/, '')}/${path}`
}

export function valueAt(data: Json, pointer: string): Json | undefined {
  let at: Json | undefined = data
  for (const step of steps(pointer)) {
    if (Array.isArray(at)) at = at[Number(step)]
    else if (isRecord(at) && Object.hasOwn(at, step)) at = at[step]
    else return undefined
  }
  return at
}

/** The data with `value` at `pointer`, or without that key when `value` is undefined. Nothing of `data` is changed. */
export function withValueAt(data: Json, pointer: string, value: Json | undefined): Json {
  const path = steps(pointer)
  if (path.length === 0) return value ?? {}
  if (path.length > MAX_PATH) return data
  const put = (at: Json | undefined, depth: number): Json => {
    const step = path[depth]
    const last = depth === path.length - 1
    if (Array.isArray(at)) {
      const copy = [...at]
      const index = step === '-' ? copy.length : Number(step)
      if (!Number.isInteger(index) || index < 0 || index > copy.length) return copy
      if (last && value === undefined) copy.splice(index, 1)
      else copy[index] = last ? (value as Json) : put(copy[index], depth + 1)
      return copy
    }
    const copy: Record_ = isRecord(at) ? { ...at } : {}
    // a key of the prototype is no key of the data
    if (step === '__proto__') return copy
    if (last && value === undefined) delete copy[step]
    else copy[step] = last ? (value as Json) : put(copy[step], depth + 1)
    return copy
  }
  return put(data, 0)
}

function asComponents(list: Json | undefined, into: Map<string, Component>): void {
  if (!Array.isArray(list)) return
  for (const entry of list) {
    if (into.size >= MAX_COMPONENTS) return
    if (isRecord(entry) && typeof entry.id === 'string' && typeof entry.component === 'string') into.set(entry.id, entry as Component)
  }
}

/** The surfaces an activity's content describes, in the order they were created. */
export function surfacesOf(content: unknown): Surface[] {
  const operations = isRecord(content) ? content.a2ui_operations : undefined
  const surfaces = new Map<string, Surface>()
  if (!Array.isArray(operations)) return []
  const surface = (id: Json | undefined): Surface | undefined => {
    if (typeof id !== 'string') return undefined
    const known = surfaces.get(id)
    if (known || surfaces.size >= MAX_SURFACES) return known
    // an agent that updates a surface it never created still meant one
    const made: Surface = { id, components: new Map(), data: {} }
    surfaces.set(id, made)
    return made
  }
  for (const operation of operations.slice(0, MAX_OPERATIONS)) {
    if (!isRecord(operation)) continue
    const { createSurface, updateComponents, updateDataModel, deleteSurface } = operation
    if (isRecord(createSurface)) {
      const made = surface(createSurface.surfaceId)
      if (made) {
        if (createSurface.dataModel !== undefined) made.data = createSurface.dataModel
        asComponents(createSurface.components, made.components)
      }
    } else if (isRecord(updateComponents)) {
      const found = surface(updateComponents.surfaceId)
      if (found) asComponents(updateComponents.components, found.components)
    } else if (isRecord(updateDataModel)) {
      const found = surface(updateDataModel.surfaceId)
      // 1.0 deletes with a null value, 0.9 with none
      const value = updateDataModel.value === null ? undefined : updateDataModel.value
      const path = typeof updateDataModel.path === 'string' ? updateDataModel.path : '/'
      if (found) found.data = withValueAt(found.data, path, value)
    } else if (isRecord(deleteSurface) && typeof deleteSurface.surfaceId === 'string') {
      surfaces.delete(deleteSurface.surfaceId)
    }
  }
  return [...surfaces.values()]
}

/** The path a value is bound to, when it is a binding (`{ path }` in 0.9, `{ "@path" }` in 1.0). */
export function boundPath(value: Json | undefined): string | undefined {
  if (!isRecord(value)) return undefined
  const path = value['@path'] ?? value.path
  return typeof path === 'string' ? path : undefined
}

/** The longest text one property shows: a component bound to all of the data does not write all of it out. */
const MAX_TEXT = 20_000
const asText = (value: Json | undefined): string =>
  (value === null || value === undefined ? '' : typeof value === 'object' ? JSON.stringify(value) : String(value)).slice(0, MAX_TEXT)

/** The most `${path}` a text may hold: each one reads the data, and a text of nothing else would read it without end. */
const MAX_REFERENCES = 64

/** `Hello ${/user/name}`: each `${path}` is the data at that path. A function call inside one is left empty. */
function interpolate(template: string, data: Json, scope: Scope): string {
  let read = 0
  return template.replace(/\\\$\{|\$\{([^}]*)\}/g, (whole, path: string | undefined) => {
    if (path === undefined) return '${'
    if (!/^[\w/~.-]+$/.test(path.trim()) || read >= MAX_REFERENCES) return ''
    read += 1
    const value = valueAt(data, absolute(path.trim(), scope))
    // a text is made of texts and numbers: a list or an object has no place in a sentence
    return typeof value === 'object' ? '' : asText(value)
  })
}

/**
 * What a property of a component is now: itself when it is plain, the data at its path when it is bound, and for
 * the one function every catalog has (`formatString`) the text it makes. Another function has no value here.
 */
export function resolve(value: Json | undefined, data: Json, scope: Scope = ''): Json | undefined {
  if (!isRecord(value)) return value
  const path = boundPath(value)
  if (path !== undefined) return valueAt(data, absolute(path, scope))
  const call = value['@call'] ?? value.call
  if (typeof call === 'string') {
    const args = isRecord(value.args) ? value.args : {}
    if (call === 'formatString') return interpolate(asText(resolve(args.value, data, scope)), data, scope)
    return undefined
  }
  return value
}

export const textOf = (value: Json | undefined, data: Json, scope: Scope = ''): string => asText(resolve(value, data, scope))

/** A child of a component, with where its relative paths start. */
export interface Child {
  id: string
  scope: Scope
}

/** The children of a component: the ids it lists, or its template once for each item of the list it names. */
export function childrenOf(children: Json | undefined, data: Json, scope: Scope): Child[] {
  if (Array.isArray(children)) {
    return children
      .slice(0, MAX_CHILDREN)
      .filter((id) => typeof id === 'string')
      .map((id) => ({ id, scope }))
  }
  if (!isRecord(children) || typeof children.componentId !== 'string' || typeof children.path !== 'string') return []
  const base = absolute(children.path, scope)
  const items = valueAt(data, base)
  // only the keys that are drawn are made: the list may be millions long
  const keys: string[] = []
  if (Array.isArray(items)) {
    for (let i = 0; i < Math.min(items.length, MAX_CHILDREN); i++) keys.push(String(i))
  } else if (isRecord(items)) {
    for (const key in items) {
      if (keys.length >= MAX_CHILDREN) break
      if (Object.hasOwn(items, key)) keys.push(key)
    }
  }
  return keys.map((key) => ({ id: children.componentId as string, scope: `${base}/${key}` }))
}

/** What goes back to the agent when the person presses a button: the action's name and its context, resolved. */
export interface UserAction {
  name: string
  surfaceId: string
  sourceComponentId: string
  timestamp: string
  context: Record_
}

const records = (list: Json | undefined): Record_[] => (Array.isArray(list) ? list.filter(isRecord).slice(0, MAX_CHILDREN) : [])

/** The tabs of a Tabs that can be shown: those that name a child. */
const shownTabs = (component: Component): Record_[] => records(component.tabs).filter((tab) => typeof tab.child === 'string')

export function tabsOf(component: Component, data: Json, scope: Scope): { title: string; child: string }[] {
  return shownTabs(component).map((tab) => ({ title: textOf(tab.title, data, scope), child: tab.child as string }))
}

/** The options of a ChoicePicker that can be chosen: those that have a value. */
export function optionsOf(component: Component, data: Json, scope: Scope): { value: string; label: string }[] {
  return records(component.options).flatMap((option) =>
    typeof option.value === 'string' ? [{ value: option.value, label: textOf(option.label, data, scope) || option.value }] : []
  )
}

/** The components a component draws inside itself, each with the scope it is drawn in. */
function inside(component: Component, data: Json, scope: Scope): Child[] {
  // read as the drawing reads it (`String(id)`), so nothing is drawn that was not counted
  const one = (id: Json | undefined): Child[] => [{ id: String(id), scope }]
  switch (component.component) {
    case 'Row':
    case 'Column':
    case 'List':
      return childrenOf(component.children, data, scope)
    case 'Card':
    case 'Button':
      return one(component.child)
    case 'Modal':
      return [...one(component.trigger), ...one(component.content)]
    case 'Tabs':
      // the children only: the count reads no title
      return shownTabs(component).map((tab) => ({ id: tab.child as string, scope }))
    default:
      return []
  }
}

/**
 * Whether the surface can be drawn within what a window draws. A component may be named by many others, so a few
 * components can ask for a drawing that never ends: the count is of what is drawn, and it stops at the limit.
 */
export function drawable(surface: Surface, data: Json): boolean {
  let left = MAX_DRAWN
  const count = (id: string, scope: Scope, depth: number): boolean => {
    const component = surface.components.get(id)
    if (!component || depth >= MAX_DEPTH) return true
    left -= 1
    if (left < 0) return false
    return inside(component, data, scope).every((child) => count(child.id, child.scope, depth + 1))
  }
  return count('root', '', 0)
}

/** The action of a component as the message for the agent, or null when it has none the agent hears of. */
export function userAction(surface: Surface, component: Component, data: Json, scope: Scope, now: Date): UserAction | null {
  const event = isRecord(component.action) ? component.action.event : undefined
  if (!isRecord(event) || typeof event.name !== 'string') return null
  const context: Record_ = {}
  if (isRecord(event.context)) for (const [key, value] of Object.entries(event.context)) context[key] = resolve(value, data, scope) ?? null
  return { name: event.name, surfaceId: surface.id, sourceComponentId: component.id, timestamp: now.toISOString(), context }
}

/** A URL an image, a video or a sound may be loaded from: the web's, or data in the URL itself. */
export function mediaUrl(value: Json | undefined): string | undefined {
  return typeof value === 'string' && /^(https?:|data:(image|audio|video)\/)/i.test(value) ? value : undefined
}
