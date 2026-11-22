/** `add_task` and `addTask` both become `AddTask`: the name of the generated types of a method. */
export function pascal(name: string): string {
  const words = name.split('_').filter((w) => w !== '')
  return words.map((w) => w.charAt(0).toUpperCase() + w.slice(1)).join('') || name
}

/**
 * Words that mean something to JavaScript or TypeScript where a member name is written in a type: reserved words, strict-mode
 * and contextual keywords. A generated member whose name is one of these is quoted (`"new"(params): ...` is a method named
 * `new`; `new(params): ...` would be a construct signature), so a name is never read as anything but a name.
 */
export const JS_WORDS = new Set([
  'abstract', 'accessor', 'any', 'as', 'asserts', 'async', 'await', 'bigint', 'boolean', 'break', 'case', 'catch', 'class', 'const', 'constructor',
  'continue', 'debugger', 'declare', 'default', 'delete', 'do', 'else', 'enum', 'export', 'extends', 'false', 'finally', 'for', 'from', 'function',
  'get', 'global', 'if', 'implements', 'import', 'in', 'infer', 'instanceof', 'interface', 'is', 'keyof', 'let', 'module', 'namespace', 'never',
  'new', 'null', 'number', 'object', 'of', 'out', 'override', 'package', 'private', 'protected', 'public', 'readonly', 'require', 'return',
  'satisfies', 'set', 'static', 'string', 'super', 'switch', 'symbol', 'this', 'throw', 'true', 'try', 'type', 'typeof', 'undefined', 'unique',
  'unknown', 'using', 'var', 'void', 'while', 'with', 'yield'
])

/** A member name as a TypeScript type writes it: bare, or quoted when it is a word the language reads as syntax. */
export const tsKey = (name: string): string => (JS_WORDS.has(name) ? JSON.stringify(name) : name)
