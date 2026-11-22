/**
 * What is wrong with a new project as it is typed, in the backend's own sentences (server/store.luau), so a mistake is
 * said at the field before anything is sent. The backend checks again, and alone knows whether a key is taken.
 */

/** Null when the name will do. Blank is not a problem to report: there is nothing to add yet. */
export function nameProblem(name: string): string | null {
  return name.trim().length > 60 ? 'A name is at most 60 characters.' : null
}

/** Null when the key will do, or is still empty. */
export function keyProblem(key: string): string | null {
  const text = key.trim().toUpperCase()
  if (text === '') return null
  return /^[A-Z][A-Z0-9]{1,4}$/.test(text) ? null : 'A key is two to five letters or digits, a letter first, like WEB.'
}
