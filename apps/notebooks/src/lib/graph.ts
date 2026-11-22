/**
 * Where the notes of a notebook sit in its graph: a dot for each note, a line for each link, laid out so that notes
 * that link to each other are near each other. The layout is worked out the same way each time (no chance in it), so
 * the same notes and links always draw the same graph.
 */

export interface GraphNode {
  readonly id: number
  readonly title: string
}
export interface GraphLink {
  readonly from: number
  readonly to: number
}
export interface Placed extends GraphNode {
  readonly x: number
  readonly y: number
  /** How many links it has, in or out: a note with more is drawn larger. */
  readonly degree: number
}
export interface Graph {
  readonly nodes: readonly Placed[]
  /** The links between the notes that are placed, as the two ends of a line. */
  readonly edges: readonly { readonly from: Placed; readonly to: Placed }[]
}

const ROUNDS = 240
/** Past this many notes a round compares too many pairs: the notes stay on their ring. */
const LIMIT = 400

/**
 * Place the notes in a box `width` by `height`, clear of its edges by `margin`. Notes push each other apart, a link
 * pulls its two ends together, and everything is drawn towards the middle, so notes without links stay in sight.
 */
export function layout(nodes: readonly GraphNode[], links: readonly GraphLink[], width: number, height: number, margin = 40): Graph {
  const count = nodes.length
  // start on a ring, in the order given: the golden angle keeps neighbours in the list apart
  const at = nodes.map((_, i) => {
    const angle = i * 2.399963229728653
    const radius = Math.sqrt((i + 0.5) / Math.max(count, 1))
    return { x: Math.cos(angle) * radius, y: Math.sin(angle) * radius }
  })
  const index = new Map(nodes.map((node, i) => [node.id, i]))
  const pairs = links.flatMap((link) => {
    const a = index.get(link.from)
    const b = index.get(link.to)
    return a === undefined || b === undefined || a === b ? [] : [[a, b] as const]
  })
  const degree = nodes.map(() => 0)
  for (const [a, b] of pairs) {
    degree[a] = (degree[a] ?? 0) + 1
    degree[b] = (degree[b] ?? 0) + 1
  }

  const ideal = 1 / Math.sqrt(Math.max(count, 1))
  for (let round = 0; round < (count <= LIMIT ? ROUNDS : 0); round++) {
    const heat = 0.1 * (1 - round / ROUNDS)
    const push = at.map(() => ({ x: 0, y: 0 }))
    for (let a = 0; a < count; a++) {
      for (let b = a + 1; b < count; b++) {
        const pa = at[a]
        const pb = at[b]
        const fa = push[a]
        const fb = push[b]
        if (!pa || !pb || !fa || !fb) continue
        const dx = pa.x - pb.x
        const dy = pa.y - pb.y
        const distance = Math.max(Math.hypot(dx, dy), 0.001)
        const force = (ideal * ideal) / distance
        fa.x += (dx / distance) * force
        fa.y += (dy / distance) * force
        fb.x -= (dx / distance) * force
        fb.y -= (dy / distance) * force
      }
    }
    for (const [a, b] of pairs) {
      const pa = at[a]
      const pb = at[b]
      const fa = push[a]
      const fb = push[b]
      if (!pa || !pb || !fa || !fb) continue
      const dx = pa.x - pb.x
      const dy = pa.y - pb.y
      const distance = Math.max(Math.hypot(dx, dy), 0.001)
      const force = (distance * distance) / ideal
      fa.x -= (dx / distance) * force
      fa.y -= (dy / distance) * force
      fb.x += (dx / distance) * force
      fb.y += (dy / distance) * force
    }
    at.forEach((p, i) => {
      const f = push[i]
      if (!f) return
      // towards the middle, and a note with no link all the more: nothing else holds it near the others
      const pull = ideal * ((degree[i] ?? 0) === 0 ? 4 : 1.5)
      f.x -= p.x * pull
      f.y -= p.y * pull
      const size = Math.max(Math.hypot(f.x, f.y), 0.001)
      p.x += (f.x / size) * Math.min(size, heat)
      p.y += (f.y / size) * Math.min(size, heat)
    })
  }

  // fit what was laid out to the box, in its own proportions: the distances between notes are the layout's
  const xs = at.map((p) => p.x)
  const ys = at.map((p) => p.y)
  const [minX, maxX, minY, maxY] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)]
  const scale = Math.min(
    Math.max(width - 2 * margin, 0) / Math.max(maxX - minX, 0.0001),
    Math.max(height - 2 * margin, 0) / Math.max(maxY - minY, 0.0001)
  )
  const fit = (value: number, min: number, max: number, room: number): number =>
    max - min < 0.0001 ? room / 2 : room / 2 + (value - (min + max) / 2) * scale
  const placed = nodes.map((node, i): Placed => {
    const p = at[i] ?? { x: 0, y: 0 }
    return { ...node, x: fit(p.x, minX, maxX, width), y: fit(p.y, minY, maxY, height), degree: degree[i] ?? 0 }
  })
  const edges = pairs.flatMap(([a, b]) => {
    const from = placed[a]
    const to = placed[b]
    return from && to ? [{ from, to }] : []
  })
  return { nodes: placed, edges }
}
