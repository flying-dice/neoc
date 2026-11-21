/**
 * Text helpers for two-way text bindings. The HOST decides conflicts and merges on the base it last
 * sent; the client reproduces the same rule where the spec leaves a merge to it:
 *
 *  - `rebase`: if the user kept typing while a write was in flight, the next write must carry both the ack's
 *    merged content and the newer keystrokes, so the draft is rebased onto the ack with the same three-way rule
 *    (base = what was sent, theirs = the ack, mine = the draft);
 *  - `mapCaret`: after an ack replaces the draft with the host's merged value, the caret and selection are mapped
 *    through the diff so they do not jump.
 *
 * `merge3` is a port of `crates/vos-domain/src/merge.rs`, token for token, so host and client agree on every
 * corner case: it works on tokens (a run of word characters, a run of blanks, or one other character), diffs them
 * with Myers' algorithm, composes edits to different tokens (insertions at one point compose, theirs first), and
 * resolves overlapping edits to MINE (the human wins) while reporting the value that lost. Keep the two in step:
 * `textMerge.test.ts` mirrors the Rust tests.
 */

/** Past this many edits the diff gives up and treats the differing middle as one edit (bounds time and memory). */
const MAX_EDIT_DISTANCE = 2_000

export interface Merge {
  /** The merged text. */
  text: string
  /** At least one region was edited on both sides differently; it resolved to `mine`. */
  conflict: boolean
  /** On a conflict, the value that lost (`theirs`): the host records it as a revision. */
  lost: string | null
}

/** A replacement of `base[aStart..aEnd]` by `other[bStart..bEnd]` (token indexes). */
interface Hunk {
  aStart: number
  aEnd: number
  bStart: number
  bEnd: number
}

type Side = 'theirs' | 'mine'

const isInsertion = (h: Hunk): boolean => h.aStart === h.aEnd

/** Processing order: by position in `base`; at one position an insertion goes before a replacement; else theirs first. */
const order = (theirs: Hunk, mine: Hunk): number =>
  theirs.aStart - mine.aStart || Number(isInsertion(mine)) - Number(isInsertion(theirs))

/** Do two edits share a base token (or does one insert strictly inside the other)? Edits that merely touch do not. */
const overlaps = (a: Hunk, b: Hunk): boolean => a.aStart < b.aEnd && b.aStart < a.aEnd

/** `base[lo..hi]` with `hunks` (all inside `[lo, hi)`, ordered, from one side) applied. */
function apply(base: string[], lo: number, hi: number, hunks: Hunk[], other: string[]): string {
  let out = ''
  let pos = lo
  for (const h of hunks) {
    out += base.slice(pos, h.aStart).join('')
    out += other.slice(h.bStart, h.bEnd).join('')
    pos = h.aEnd
  }
  return out + base.slice(pos, hi).join('')
}

/**
 * Merge `mine` and `theirs`, both derived from `base`: `base` is what the writer last saw, `theirs` the current
 * value (usually an agent's write), `mine` the writer's draft.
 */
export function merge3(base: string, theirs: string, mine: string): Merge {
  const clean = (text: string): Merge => ({ text, conflict: false, lost: null })
  if (mine === theirs || theirs === base) return clean(mine)
  if (mine === base) return clean(theirs)
  const b = tokenize(base)
  const t = tokenize(theirs)
  const m = tokenize(mine)
  const their = diff(b, t)
  const my = diff(b, m)
  let ti = 0
  let mi = 0
  let out = ''
  let pos = 0 // next base token not yet emitted
  let conflict = false
  for (;;) {
    const th = their[ti]
    const mh = my[mi]
    if (!th && !mh) break
    const side: Side = !th || (mh && order(th, mh) > 0) ? 'mine' : 'theirs'
    const hunk = side === 'mine' ? mh : th
    out += b.slice(pos, hunk.aStart).join('')

    // is the next hunk of the OTHER side overlapping this one? then resolve the whole cluster
    const other = side === 'theirs' ? my[mi] : their[ti]
    if (!(other && overlaps(hunk, other))) {
      const tokens = side === 'theirs' ? t : m
      out += tokens.slice(hunk.bStart, hunk.bEnd).join('')
      pos = hunk.aEnd
      if (side === 'theirs') ti++
      else mi++
      continue
    }

    // cluster: grow [lo, hi) while any next hunk (either side) overlaps it
    const lo = hunk.aStart
    let hi = hunk.aEnd
    const theirsIn: Hunk[] = []
    const mineIn: Hunk[] = []
    if (side === 'theirs') theirsIn.push(their[ti++])
    else mineIn.push(my[mi++])
    for (let grew = true; grew;) {
      grew = false
      while (their[ti] && overlaps({ aStart: lo, aEnd: hi, bStart: 0, bEnd: 0 }, their[ti])) {
        hi = Math.max(hi, their[ti].aEnd)
        theirsIn.push(their[ti++])
        grew = true
      }
      while (my[mi] && overlaps({ aStart: lo, aEnd: hi, bStart: 0, bEnd: 0 }, my[mi])) {
        hi = Math.max(hi, my[mi].aEnd)
        mineIn.push(my[mi++])
        grew = true
      }
    }
    const theirsRegion = apply(b, lo, hi, theirsIn, t)
    const mineRegion = apply(b, lo, hi, mineIn, m)
    if (theirsRegion !== mineRegion) conflict = true
    out += mineRegion // the human wins
    pos = hi
  }
  out += b.slice(pos).join('')
  return { text: out, conflict, lost: conflict ? theirs : null }
}

type Class = 'word' | 'blank' | 'other'
const WORD = /[\p{Alphabetic}\p{N}_]/u
const classOf = (ch: string): Class =>
  WORD.test(ch) ? 'word' : ch === ' ' || ch === '\t' ? 'blank' : 'other'

/** Split into tokens: a run of alphanumerics/underscores, a run of spaces/tabs, or one other character. */
export function tokenize(s: string): string[] {
  const tokens: string[] = []
  let start = 0
  let prev: Class | null = null
  for (let i = 0; i < s.length;) {
    const ch = String.fromCodePoint(s.codePointAt(i)!)
    const cls = classOf(ch)
    const continues = prev !== null && prev === cls && cls !== 'other'
    if (!continues && i > start) {
      tokens.push(s.slice(start, i))
      start = i
    }
    prev = cls
    i += ch.length
  }
  if (start < s.length) tokens.push(s.slice(start))
  return tokens
}

/** The edits turning `a` into `b`, as ordered non-adjacent hunks. */
export function diff(a: string[], b: string[]): Hunk[] {
  let prefix = 0
  while (prefix < a.length && prefix < b.length && a[prefix] === b[prefix]) prefix++
  let suffix = 0
  while (
    suffix < a.length - prefix &&
    suffix < b.length - prefix &&
    a[a.length - 1 - suffix] === b[b.length - 1 - suffix]
  )
    suffix++
  const aMid = a.slice(prefix, a.length - suffix)
  const bMid = b.slice(prefix, b.length - suffix)
  if (aMid.length === 0 && bMid.length === 0) return []
  const hunks = myers(aMid, bMid) ?? [
    { aStart: 0, aEnd: aMid.length, bStart: 0, bEnd: bMid.length }
  ]
  return hunks.map((h) => ({
    aStart: h.aStart + prefix,
    aEnd: h.aEnd + prefix,
    bStart: h.bStart + prefix,
    bEnd: h.bEnd + prefix
  }))
}

/** Myers' O(ND) diff. Null when the edit distance exceeds `MAX_EDIT_DISTANCE`. */
function myers(a: string[], b: string[]): Hunk[] | null {
  const n = a.length
  const m = b.length
  const max = Math.min(n + m, MAX_EDIT_DISTANCE)
  const offset = max + 1
  const v: number[] = new Array<number>(2 * max + 3).fill(0)
  const trace: number[][] = []
  let found: number | null = null
  search: for (let d = 0; d <= max; d++) {
    trace.push(v.slice())
    for (let k = -d; k <= d; k += 2) {
      const down = k === -d || (k !== d && v[offset + k - 1] < v[offset + k + 1])
      let x = down ? v[offset + k + 1] : v[offset + k - 1] + 1
      let y = x - k
      if (y < 0) return null
      while (x < n && y < m && a[x] === b[y]) {
        x++
        y++
      }
      v[offset + k] = x
      if (x >= n && y >= m) {
        found = d
        break search
      }
    }
  }
  if (found === null) return null
  // backtrack into per-token edits, then group into hunks
  const edits: [number, number, boolean][] = [] // (x, y, isDelete): a deletion at a[x] or insertion of b[y]
  let x = n
  let y = m
  for (let d = found; d >= 1; d--) {
    const vv = trace[d]
    const k = x - y
    const down = k === -d || (k !== d && vv[offset + k - 1] < vv[offset + k + 1])
    const prevK = down ? k + 1 : k - 1
    const prevX = vv[offset + prevK]
    const prevY = prevX - prevK
    if (prevY < 0) return null
    while (x > prevX && y > prevY) {
      x--
      y--
    }
    if (down)
      edits.push([x, y - 1, false]) // insertion of b[y-1] at a-position x
    else edits.push([x - 1, y, true]) // deletion of a[x-1]
    x = prevX
    y = prevY
  }
  edits.reverse()
  return group(edits)
}

/** Group single-token edits into hunks (consecutive in both sequences). */
function group(edits: [number, number, boolean][]): Hunk[] {
  const hunks: Hunk[] = []
  for (const [x, y, isDelete] of edits) {
    const [aLo, aHi, bLo, bHi] = isDelete ? [x, x + 1, y, y] : [x, x, y, y + 1]
    const last = hunks.at(-1)
    if (last && last.aEnd === aLo && last.bEnd === bLo) {
      last.aEnd = Math.max(aHi, last.aEnd)
      last.bEnd = Math.max(bHi, last.bEnd)
    } else hunks.push({ aStart: aLo, aEnd: aHi, bStart: bLo, bEnd: bHi })
  }
  return hunks
}

// ── Caret mapping and rebasing: character level, the client's own concern ──

export interface CharHunk {
  at: number
  del: number
  ins: string
}

/** The single edit that turns `a` into `b` (common prefix and suffix trimmed). */
export function diff1(a: string, b: string): CharHunk {
  let p = 0
  while (p < a.length && p < b.length && a[p] === b[p]) p++
  let s = 0
  while (s < a.length - p && s < b.length - p && a[a.length - 1 - s] === b[b.length - 1 - s]) s++
  return { at: p, del: a.length - p - s, ins: b.slice(p, b.length - s) }
}

/** Where a caret at `pos` in `before` lands in `after` (only the changed middle moves it). */
export function mapCaret(before: string, after: string, pos: number): number {
  const { at, del, ins } = diff1(before, after)
  if (pos <= at) return pos
  if (pos >= at + del) return pos - del + ins.length
  return at + ins.length // inside the replaced span: the end of the replacement
}

/**
 * The draft after a write was acked with a merged value while the user kept typing: `sent` is what was
 * written, `ack` what the host answered, `draft` what is in the field now.
 */
export function rebase(sent: string, ack: string, draft: string): string {
  if (ack === sent) return draft
  return merge3(sent, ack, draft).text
}
