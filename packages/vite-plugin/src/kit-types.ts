/**
 * Editor types for the kit's tags, generated from its Custom Elements Manifest (`packages/kit/custom-elements.json`,
 * written by `bun run build:kit` and committed; the plugin writes them into an app's `types/` on every build). An app that writes `<ui-button varient="x">` or `onchange={...}` with the wrong
 * payload is flagged by its editor and its type check, in Svelte (svelte-check) and in React (tsc).
 *
 * Types only: nothing here is shipped to a frame, and the app's bundle never contains a second copy of the kit
 * (`/kit/kit.js` is the one that runs).
 *
 * The output is a module that augments the framework's own JSX typing, so it is two files, one per framework, and an app
 * gets the one for the framework its `package.json` depends on (`frameworksOf`):
 *   svelte  -> `svelte/elements`' `SvelteHTMLElements`
 *   react   -> React's `JSX.IntrinsicElements` (React 19 types)
 */

interface Attribute {
  name: string
  type?: { text: string }
  description?: string
}
interface Event {
  name: string
  type?: { text: string }
  description?: string
}
interface Declaration {
  kind: string
  tagName?: string
  customElement?: boolean
  description?: string
  attributes?: Attribute[]
  events?: Event[]
  slots?: { name: string; description?: string }[]
}
export interface Manifest {
  modules: { declarations?: Declaration[] }[]
}

export type Framework = 'svelte' | 'react'

/** The kit's elements, in tag order. */
export function elementsOf(manifest: Manifest): Declaration[] {
  const found = manifest.modules.flatMap((m) => m.declarations ?? []).filter((d) => d.customElement && d.tagName)
  return found.sort((a, b) => a.tagName!.localeCompare(b.tagName!))
}

/** One line of JSDoc from a manifest description (so the editor shows it on hover). */
const doc = (text: string | undefined, indent: string): string => {
  const clean = (text ?? '').replace(/\*\//g, '* /').replace(/\s+/g, ' ').trim()
  return clean ? `${indent}/** ${clean} */\n` : ''
}

/** A TypeScript property name for an attribute or a handler. */
const key = (name: string): string => (/^[A-Za-z_$][\w$]*$/.test(name) ? name : JSON.stringify(name))

function propsOf(el: Declaration, events: Event[], indent: string): string {
  const inner = `${indent}  `
  const lines: string[] = []
  for (const a of el.attributes ?? []) lines.push(`${doc(a.description, inner)}${inner}${key(a.name)}?: ${a.type?.text ?? 'string'}`)
  for (const e of events) lines.push(`${doc(e.description, inner)}${inner}on${e.name}?: (event: ${e.type?.text ?? 'Event'}) => void`)
  return `{\n${lines.join('\n')}\n${indent}}`
}

const HEADER = '// Generated from the kit\'s custom-elements.json by the vos Vite plugin (packages/vite-plugin/src/kit-types.ts). Do not edit.\n'

/** The declaration file for one framework. */
export function kitTypes(manifest: Manifest, framework: Framework): string {
  const elements = elementsOf(manifest)
  if (framework === 'svelte') {
    const entries = elements.map((el) => {
      const events = (el.events ?? []).filter((e) => e.name !== 'click')
      const overridden = events.map((e) => `'on${e.name}'`)
      const base = overridden.length ? `Omit<HTMLAttributes<HTMLElement>, ${overridden.join(' | ')}>` : 'HTMLAttributes<HTMLElement>'
      const props = propsOf(el, events, '    ')
      return `${doc(el.description, '    ')}    ${key(el.tagName!)}: ${base} & ${props}`
    })
    return `${HEADER}import type { HTMLAttributes } from 'svelte/elements'

declare module 'svelte/elements' {
  interface SvelteHTMLElements {
${entries.join('\n')}
  }
}

export {}
`
  }
  const entries = elements.map((el) => {
    // React 19 gives a custom element `on<name>` as a native listener for <name> when React has no event of that name, so
    // `onchange` hears `change`. `onChange` and `onInput` are React's own synthetic events, which an element that is not
    // an input never raises: they are left out of the type, so a wrong one is an error rather than a silent no-op.
    // (`onClick` is fine: click is delegated.)
    const events = (el.events ?? []).filter((e) => e.name !== 'click')
    const dropped = events.filter((e) => e.name === 'change' || e.name === 'input').map((e) => `'on${e.name[0]!.toUpperCase()}${e.name.slice(1)}'`)
    const plain = 'React.DetailedHTMLProps<React.HTMLAttributes<HTMLElement>, HTMLElement>'
    const base = dropped.length ? `Omit<${plain}, ${dropped.join(' | ')}>` : plain
    const props = propsOf(el, events, '      ')
    return `${doc(el.description, '      ')}      ${key(el.tagName!)}: ${base} & ${props}`
  })
  return `${HEADER}import type * as React from 'react'

declare module 'react' {
  namespace JSX {
    interface IntrinsicElements {
${entries.join('\n')}
    }
  }
}

export {}
`
}

/**
 * The frameworks a project's editor types are made for: those its `package.json` depends on (`dependencies` or
 * `devDependencies`), Svelte and React in that order. Both is both files; neither (a plain TypeScript or JavaScript app)
 * is none: the kit's elements still work there, typed as `HTMLElement` through `document.querySelector`.
 */
export function frameworksOf(packageJson: { dependencies?: Record<string, string>; devDependencies?: Record<string, string> }): Framework[] {
  const deps = { ...packageJson.dependencies, ...packageJson.devDependencies }
  return (['svelte', 'react'] as const).filter((framework) => framework in deps)
}
