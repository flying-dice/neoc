/**
 * Every path the contract names (today: the migrations of `x-app`) is input from whoever wrote the app, and a build must
 * never read or write outside the project and its `dist/`. One place decides: the shape is checked when the contract is read
 * ({@link relativeFile}), and every read or write resolves the name against its root with the platform's own routines and
 * checks the result stays inside ({@link inside}), following symbolic links for what exists.
 */
import { existsSync, realpathSync } from 'node:fs'
import { isAbsolute, join, relative, resolve, sep } from 'node:path'

/** A file name as the contract may give it: relative, plain segments (letters, digits, `.`, `_`, `-`), no `..`, nothing hidden. */
const SEGMENT = /^[A-Za-z0-9_][A-Za-z0-9._-]*$/

/** Why `value` is not a relative file name the build may use, or `null` when it is. */
export function refuseRelativeFile(value: string): string | null {
  if (value === '') return 'is empty'
  if (value.includes('\0')) return 'has a NUL character'
  if (value.includes('\\')) return 'uses a backslash: write it with `/`'
  if (isAbsolute(value) || value.startsWith('/') || /^[A-Za-z]:/.test(value)) return 'is an absolute path: a file is named relative to the project'
  for (const segment of value.split('/')) {
    if (segment === '..') return 'climbs out of the project with `..`'
    if (segment === '' || segment === '.') return 'has an empty or `.` segment'
    if (!SEGMENT.test(segment)) return `has a segment \`${segment}\` that is hidden or has a character other than letters, digits, \`.\`, \`_\` and \`-\``
  }
  return null
}

/**
 * `name` resolved against `root`, as an absolute path that is certain to be inside `root`: the platform's `resolve` does the
 * joining, then the result (and, where it exists, its real path) is checked with `relative`. Throws when it is not.
 */
export function inside(root: string, name: string): string {
  const base = resolve(root)
  const target = resolve(base, name)
  if (!contained(base, target)) throw new Error(`\`${name}\` is outside ${base}: a build reads and writes only inside the project and its dist`)
  // a symbolic link in the way must not lead out either: judge the real path of the nearest part that exists
  let existing = target
  while (!existsSync(existing) && existing !== base) existing = resolve(existing, '..')
  if (existsSync(existing) && !contained(realpathSync(base), realpathSync(existing))) {
    throw new Error(`\`${name}\` resolves outside ${base} through a symbolic link: a build reads and writes only inside the project and its dist`)
  }
  return target
}

/** Whether `target` is `base` or inside it (both absolute). */
export function contained(base: string, target: string): boolean {
  const from = relative(base, target)
  return from === '' || (!from.startsWith('..') && !isAbsolute(from) && !from.split(sep).includes('..'))
}

/** The names of a path's segments joined for the app ref: always `/`. */
export const asUrlPath = (name: string): string => name.split(sep).join('/')

/** `join` that cannot leave `root`. */
export const joinInside = (root: string, ...parts: string[]): string => inside(root, join(...parts))
