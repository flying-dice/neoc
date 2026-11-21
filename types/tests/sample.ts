/**
 * Type-level contract: a sample tree is checked against the generated types (`bun run typecheck` runs
 * `tsc -p types`). The `@ts-expect-error` lines prove wrong trees are rejected: if the generated types stop
 * constraining them, the directive is unused and tsc fails.
 */
import type {
  Action,
  ButtonNode,
  DataPatch,
  Expr,
  Node,
  Op,
  SourceDecl,
  TextFieldNode,
  TypedNode,
  ViewOps,
  ViewSnapshot
} from '../generated/vos'

const button: ButtonNode = {
  type: 'Button',
  key: 'go',
  props: { title: 'Go', role: 'primary', fill: 'orange', selected: { $ref: 'client.pressed' } },
  on: { tap: true }
}

const search: TextFieldNode = {
  type: 'TextField',
  key: 'q',
  props: { text: { $ref: 'client.search' }, placeholder: 'Search', sync: 'debounce:250' },
  on: { submit: [{ do: 'set', ref: 'client.q', value: 'x' }] }
}

export const tree: TypedNode = {
  type: 'Stack',
  key: 'main',
  props: { axis: 'v', spacing: 2, padding: { x: 2, y: 1 } },
  children: [
    { type: 'Text', key: 'title', props: { text: { $fn: 'concat', args: [{ $ref: 'src.s_notes.total' }, ' notes'] }, style: 'title1' } },
    search,
    button,
    {
      type: 'List',
      key: 'notes',
      props: { style: 'plain', source: 's_notes', selection: { $ref: 'client.noteId' } },
      slots: { row: [{ type: 'Row', key: '_0', props: { title: { $ref: 'row.title' } } }] }
    }
  ]
}

// a wire node is loosely typed: any type name, any props
export const wire: Node = { type: 'FutureThing', key: 'x', props: { anything: [1, 2, 3] } }

export const op: Op = { op: 'insert', parent: 'main', before: null, node: wire }
export const ops: ViewOps = { viewId: 'v1', base: 3, rev: 4, ops: [op, { op: 'set', path: 'main/q', props: { placeholder: null } }] }
export const patch: DataPatch = {
  viewId: 'v1',
  sources: { s_notes: { kind: 'rows', total: 2, from: 0, keys: [1, 2], rows: { '1': { id: 1 } } } }
}
export const source: SourceDecl = { kind: 'rows', key: 'id', params: { q: { $ref: 'client.search' } } }
export const action: Action = { do: 'nav.push', route: { name: 'note', id: 7 } }
export const expr: Expr<'a' | 'b'> = { $fn: 'if', args: [true, 'a', 'b'] }
export const snapshot: Pick<ViewSnapshot, 'rev' | 'tree'> = { rev: 1, tree: wire }

// ---- wrong trees are rejected ---------------------------------------------------------------------------

// @ts-expect-error a prop the component does not have
export const unknownProp: ButtonNode = { type: 'Button', key: 'a', props: { title: 'x', colour: 'red' } }

// @ts-expect-error an enum value outside the closed set
export const badEnum: ButtonNode = { type: 'Button', key: 'a', props: { shape: 'blob' } }

// @ts-expect-error a binding is a ref, not a value
export const literalBinding: TextFieldNode = { type: 'TextField', key: 'q', props: { text: 'literal' } }

// @ts-expect-error a required prop is missing (Text needs `text`)
export const missingRequired: TypedNode = { type: 'Text', key: 't', props: { style: 'body' } }

// @ts-expect-error an event the component does not report
export const unknownEvent: ButtonNode = { type: 'Button', key: 'a', on: { change: true } }

// @ts-expect-error spacing is 0..8
export const badSpacing: TypedNode = { type: 'Stack', key: 's', props: { spacing: 9 } }

// @ts-expect-error a function outside the closed set
export const badFn: Expr<string> = { $fn: 'eval', args: [] }

// @ts-expect-error an unknown op
export const badOp: Op = { op: 'splice', path: 'a' }
