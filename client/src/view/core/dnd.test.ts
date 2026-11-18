import { describe, expect, test } from 'bun:test'
import {
  DragController,
  keyboardEdge,
  parseDrag,
  parseDrop,
  pointerEdge,
  type DropEvent,
  type DropTarget,
  type Rect
} from './dnd'

function target(
  id: string,
  order: number,
  accepts: string[],
  extra: { rect?: Rect } = {}
): DropTarget & { dropped: DropEvent[] } {
  const dropped: DropEvent[] = []
  return {
    id,
    key: id,
    label: id,
    accepts,
    order: () => order,
    rect: () => extra.rect ?? null,
    holds: () => false,
    isContainer: () => false,
    drop: (e) => void dropped.push(e),
    dropped
  }
}

describe('drag and drop props', () => {
  test('a drag needs a type; payload and preview ride along', () => {
    expect(parseDrag({ type: 'task', payload: { id: 7 }, preview: 'Task 7' })).toEqual({
      type: 'task',
      payload: { id: 7 },
      preview: 'Task 7'
    })
    for (const bad of [null, 'task', { payload: 1 }, { type: '' }, { type: 3 }, []])
      expect(parseDrag(bad)).toBeNull()
  })

  test('a drop target lists the types it accepts, highlighting by default', () => {
    expect(parseDrop({ accepts: ['task', 'note', 3, ''] })).toEqual({
      accepts: ['task', 'note'],
      highlight: true
    })
    expect(parseDrop({ accepts: ['task'], highlight: false })?.highlight).toBe(false)
    for (const bad of [null, {}, { accepts: [] }, { accepts: 'task' }, { accepts: [1] }])
      expect(parseDrop(bad)).toBeNull()
  })
})

describe('drop edges', () => {
  const r: Rect = { x: 0, y: 0, width: 200, height: 100 }
  test('the middle of a container is inside; its rim and plain items are by half', () => {
    expect(pointerEdge(r, { x: 100, y: 50 }, true)).toBe('inside')
    expect(pointerEdge(r, { x: 100, y: 5 }, true)).toBe('top')
    expect(pointerEdge(r, { x: 100, y: 95 }, true)).toBe('bottom')
    expect(pointerEdge(r, { x: 100, y: 30 }, false)).toBe('top')
    expect(pointerEdge(r, { x: 100, y: 70 }, false)).toBe('bottom')
  })

  test('a very wide item splits left and right', () => {
    const wide: Rect = { x: 0, y: 0, width: 400, height: 40 }
    expect(pointerEdge(wide, { x: 50, y: 20 }, false)).toBe('leading')
    expect(pointerEdge(wide, { x: 350, y: 20 }, false)).toBe('trailing')
  })

  test('a keyboard move is inside a container, or beside an item in the direction of travel', () => {
    expect(keyboardEdge(true, 1)).toBe('inside')
    expect(keyboardEdge(true, -1)).toBe('inside')
    expect(keyboardEdge(false, 1)).toBe('bottom')
    expect(keyboardEdge(false, -1)).toBe('top')
  })
})

describe('the drag controller (keyboard and menu paths)', () => {
  function board(): {
    c: DragController
    todo: ReturnType<typeof target>
    doing: ReturnType<typeof target>
    done: ReturnType<typeof target>
    other: ReturnType<typeof target>
  } {
    const c = new DragController()
    const todo = target('todo', 10, ['task'])
    const doing = target('doing', 20, ['task'])
    const done = target('done', 30, ['task'])
    const other = target('notes', 25, ['note'])
    for (const t of [done, other, todo, doing]) c.register(t) // registered out of order on purpose
    return { c, todo, doing, done, other }
  }

  test('candidates accept the type, in document order, never the source', () => {
    const { c } = board()
    expect(c.candidates('task').map((t) => t.id)).toEqual(['todo', 'doing', 'done'])
    expect(c.candidates('task', 'doing').map((t) => t.id)).toEqual(['todo', 'done'])
    expect(c.candidates('note').map((t) => t.id)).toEqual(['notes'])
    expect(c.candidates('nothing')).toEqual([])
  })

  test('Alt+Down goes to the next target after the source, Alt+Up to the previous', () => {
    const { c } = board()
    const never = (): boolean => false
    expect(c.step('task', 'card', 12, 1, never)?.id).toBe('doing')
    expect(c.step('task', 'card', 12, -1, never)?.id).toBe('todo')
    expect(c.step('task', 'card', 25, 1, never)?.id).toBe('done')
    expect(c.step('task', 'card', 35, 1, never)).toBeNull()
    expect(c.step('task', 'card', 5, -1, never)).toBeNull()
  })

  test('the column that holds the source is not a destination', () => {
    const { c } = board()
    const holds = (t: DropTarget): boolean => t.id === 'doing'
    expect(c.step('task', 'card', 22, -1, holds)?.id).toBe('todo')
    expect(c.step('task', 'card', 22, 1, holds)?.id).toBe('done')
  })

  test('a pointer drop lands on the innermost accepting target under the point', () => {
    const c = new DragController()
    c.register(target('column', 1, ['task'], { rect: { x: 0, y: 0, width: 300, height: 600 } }))
    c.register(target('card', 2, ['task'], { rect: { x: 10, y: 10, width: 280, height: 60 } }))
    c.register(target('elsewhere', 3, ['note'], { rect: { x: 0, y: 0, width: 300, height: 600 } }))
    expect(c.hit('task', { x: 20, y: 20 })?.id).toBe('card')
    expect(c.hit('task', { x: 20, y: 300 })?.id).toBe('column')
    expect(c.hit('task', { x: 20, y: 20 }, 'card')?.id).toBe('column')
    expect(c.hit('task', { x: 900, y: 20 })).toBeNull()
  })

  test('unregistering removes the target; drag state notifies', () => {
    const c = new DragController()
    const off = c.register(target('a', 1, ['task']))
    off()
    expect(c.candidates('task')).toEqual([])
    let n = 0
    c.onChange(() => n++)
    c.start('card', 'task')
    expect(c.active).toEqual({ id: 'card', type: 'task' })
    c.end()
    expect(c.active).toBeNull()
    expect(n).toBe(2)
  })

  test('the pointer-over target is tracked, cleared on end, and notifies only on change', () => {
    const c = new DragController()
    let n = 0
    c.onChange(() => n++)
    c.setOver('a')
    c.setOver('a')
    expect(c.over).toBe('a')
    c.setOver('b')
    c.end()
    expect(c.over).toBeNull()
    expect(n).toBe(3)
  })

  test('dropping calls the target with the event', () => {
    const t = target('done', 1, ['task'])
    t.drop({
      type: 'task',
      payload: { id: 1 },
      target: { key: 'done', beforeKey: null, edge: 'inside' }
    })
    expect(t.dropped).toHaveLength(1)
    expect(t.dropped[0]?.target.edge).toBe('inside')
  })
})
