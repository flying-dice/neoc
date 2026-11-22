import { expect, test } from 'bun:test'
import { layout } from './graph'

const NODES = [1, 2, 3, 4, 5, 6].map((id) => ({ id, title: `Note ${id}` }))
const LINKS = [
  { from: 1, to: 2 },
  { from: 2, to: 3 },
  { from: 1, to: 3 }
]
const distance = (a: { x: number; y: number }, b: { x: number; y: number }): number => Math.hypot(a.x - b.x, a.y - b.y)

test('every note is placed inside the box, clear of its edges', () => {
  const graph = layout(NODES, LINKS, 600, 400, 40)
  expect(graph.nodes.map((n) => n.id)).toEqual([1, 2, 3, 4, 5, 6])
  for (const node of graph.nodes) {
    expect(node.x).toBeGreaterThanOrEqual(40)
    expect(node.x).toBeLessThanOrEqual(560)
    expect(node.y).toBeGreaterThanOrEqual(40)
    expect(node.y).toBeLessThanOrEqual(360)
  }
})

test('the same notes and links are laid out the same way each time', () => {
  expect(layout(NODES, LINKS, 600, 400)).toEqual(layout(NODES, LINKS, 600, 400))
})

test('notes that link to each other are nearer than notes that do not', () => {
  const [a, b, , d] = layout(NODES, LINKS, 600, 400).nodes
  expect(distance(a!, b!)).toBeLessThan(distance(a!, d!))
})

test('no two notes are on the same spot', () => {
  const { nodes } = layout(NODES, LINKS, 600, 400)
  for (const a of nodes) for (const b of nodes) if (a.id < b.id) expect(distance(a, b)).toBeGreaterThan(20)
})

test('a note counts its links, and a link to a note that is not there is not drawn', () => {
  const graph = layout(NODES, [...LINKS, { from: 1, to: 99 }, { from: 4, to: 4 }], 600, 400)
  expect(graph.nodes.map((n) => n.degree)).toEqual([2, 2, 2, 0, 0, 0])
  expect(graph.edges.map((e) => [e.from.id, e.to.id])).toEqual([
    [1, 2],
    [2, 3],
    [1, 3]
  ])
})

test('one note sits in the middle; none is an empty graph', () => {
  expect(layout([{ id: 1, title: 'Alone' }], [], 600, 400).nodes).toEqual([{ id: 1, title: 'Alone', x: 300, y: 200, degree: 0 }])
  expect(layout([], [], 600, 400)).toEqual({ nodes: [], edges: [] })
})
