/** What is wrong with the name typed for a new folder. The backend checks the same (server/store.luau), and has the last word. */
type Folder = VosApp.Notes.Folder

/** Why a folder cannot be called `name`, or nothing when it can. A blank name is not a problem to report: there is nothing to add yet. */
export function folderProblem(name: string, folders: readonly Folder[]): string | null {
  const named = name.trim()
  if (named === '') return null
  const taken = folders.some((f) => f.name.toLowerCase() === named.toLowerCase())
  return taken ? `There is already a folder called ${named}.` : null
}
