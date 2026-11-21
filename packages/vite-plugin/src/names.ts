/** `add_task` and `addTask` both become `AddTask`: the name of the generated types of a method. */
export function pascal(name: string): string {
  const words = name.split('_').filter((w) => w !== '')
  return words.map((w) => w.charAt(0).toUpperCase() + w.slice(1)).join('') || name
}
