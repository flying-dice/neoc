/**
 * Text helpers for two-way text bindings (spec 3.4). The HOST decides conflicts and merges on the base it last
 * sent (the fake host in `host/` uses `merge3` for that). The client uses the same primitives for two things the
 * spec leaves to it:
 *
 *  - `mapCaret`: after an ack replaces the draft with the host's merged value, the caret and selection are mapped
 *    through the diff (common prefix/suffix) so they do not jump;
 *  - `rebase`: if the user kept typing while a write was in flight, the next write must carry both the ack's
 *    merged content and the newer keystrokes, so the draft is rebased onto the ack with the same three-way rule
 *    (base = what was sent, theirs = the ack, mine = the draft).
 *
 * Single-hunk and character-level: enough for "an agent appended while the user typed elsewhere". Production
 * hosts use a real diff3; the contract (non-overlapping hunks compose, overlaps resolve to mine) is the same.
 */
export interface Hunk {
  at: number
  del: number
  ins: string
}

/** The single edit that turns `a` into `b` (common prefix and suffix trimmed). */
export function diff1(a: string, b: string): Hunk {
  let p = 0
  while (p < a.length && p < b.length && a[p] === b[p]) p++
  let s = 0
  while (s < a.length - p && s < b.length - p && a[a.length - 1 - s] === b[b.length - 1 - s]) s++
  return { at: p, del: a.length - p - s, ins: b.slice(p, b.length - s) }
}

export interface MergeResult {
  value: string
  conflict: boolean
}

/**
 * base = what the writer last saw, theirs = the current value, mine = the writer's draft. Non-overlapping hunks
 * compose; two pure insertions at the same point compose, theirs first; overlapping ones resolve to MINE (the
 * human at the keyboard wins) and report `conflict`.
 */
export function merge3(base: string, theirs: string, mine: string): MergeResult {
  if (mine === base) return { value: theirs, conflict: false }
  if (theirs === base || theirs === mine) return { value: mine, conflict: false }
  const t = diff1(base, theirs)
  const m = diff1(base, mine)
  if (t.del === 0 && m.del === 0 && t.at === m.at) {
    return { value: base.slice(0, t.at) + t.ins + m.ins + base.slice(t.at), conflict: false }
  }
  const overlap = (t.at < m.at + m.del && m.at < t.at + t.del) || t.at === m.at
  if (overlap) return { value: mine, conflict: true }
  const [first, second] = t.at < m.at ? [t, m] : [m, t]
  const value =
    base.slice(0, first.at) + first.ins + base.slice(first.at + first.del, second.at) + second.ins + base.slice(second.at + second.del)
  return { value, conflict: false }
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
  return merge3(sent, ack, draft).value
}
