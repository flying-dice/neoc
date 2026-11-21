import { describe, expect, test } from 'bun:test'
import { diff, diff1, mapCaret, merge3, rebase, tokenize } from './textMerge'

// The first block mirrors the unit tests of crates/vos-domain/src/merge.rs one for one: host and client
// must agree on every corner case.

const m3 = (base: string, theirs: string, mine: string): [string, boolean] => {
  const r = merge3(base, theirs, mine)
  return [r.text, r.conflict]
}

describe('merge3 (port of merge.rs)', () => {
  test('the spec example composes', () => {
    // the user capitalised "milk" while an agent appended a line
    const r = merge3('milk', 'milk\n- buy oat milk', 'Milk')
    expect(r.text).toBe('Milk\n- buy oat milk')
    expect(r.conflict).toBe(false)
    expect(r.lost).toBeNull()
  })

  test('trivial cases', () => {
    expect(m3('a b', 'a b', 'a b')).toEqual(['a b', false])
    expect(m3('a b', 'a c', 'a b')).toEqual(['a c', false]) // only they edited
    expect(m3('a b', 'a b', 'a d')).toEqual(['a d', false]) // only I edited
    expect(m3('a b', 'a x', 'a x')).toEqual(['a x', false]) // identical edits are not a conflict
    expect(m3('', '', '')).toEqual(['', false])
    expect(m3('', 'theirs', 'mine')).toEqual(['theirsmine', false]) // two insertions at one point compose
  })

  test('edits in different places compose, symmetrically', () => {
    const base = 'one\ntwo\nthree\nfour\n'
    const theirs = 'one\ntwo\nthree\nfour\nfive\n' // agent appended
    const mine = 'ONE\ntwo\nthree\nfour\n' // human edited the first line
    expect(m3(base, theirs, mine)).toEqual(['ONE\ntwo\nthree\nfour\nfive\n', false])
    expect(m3(base, mine, theirs)).toEqual(['ONE\ntwo\nthree\nfour\nfive\n', false])
  })

  test('same-point insertions compose, theirs first', () => {
    let r = merge3('a,b', 'a,X,b', 'a,Y,b')
    expect([r.text, r.conflict]).toEqual(['a,X,Y,b', false])
    r = merge3('ab', 'aXb', 'aYb')
    expect(r.conflict).toBe(true) // a word rewritten two ways is one edit of one token, not two insertions
    r = merge3('start end', 'start AGENT end', 'start HUMAN end')
    expect(r.text).toBe('start AGENT HUMAN end') // blanks around the insertion are shared tokens
    expect(r.conflict).toBe(false)
  })

  test('an insertion at the edge of an edit composes', () => {
    // I replace "milk", they append right after it
    expect(m3('buy milk now', 'buy milk! now', 'buy oat now')).toEqual(['buy oat! now', false])
  })

  test('overlapping edits conflict and the human wins', () => {
    const r = merge3('buy milk today', 'buy bread today', 'buy cheese today')
    expect(r.text).toBe('buy cheese today')
    expect(r.conflict).toBe(true)
    expect(r.lost).toBe('buy bread today') // their whole value is kept as a revision
  })

  test('a deletion overlapping an edit conflicts', () => {
    const r = merge3('a b c', 'a c', 'a B c')
    expect(r.conflict).toBe(true)
    expect(r.text).toBe('a B c')
  })

  test('non-overlapping hunks still compose next to a conflict', () => {
    const base = 'alpha beta\n\ngamma delta\n'
    const theirs = 'alpha BETA\n\ngamma delta\nepsilon\n' // edits beta, appends
    const mine = 'alpha Beta\n\ngamma delta\n' // edits beta differently
    const r = merge3(base, theirs, mine)
    expect(r.conflict).toBe(true)
    expect(r.text).toBe('alpha Beta\n\ngamma delta\nepsilon\n')
  })

  test('a deletion of theirs composes with my edit elsewhere', () => {
    const base = 'keep this\nremove me\nand this\n'
    const theirs = 'keep this\nand this\n'
    const mine = 'keep that\nremove me\nand this\n'
    expect(m3(base, theirs, mine)).toEqual(['keep that\nand this\n', false])
  })

  test('unicode and emoji are never split', () => {
    const r = merge3('café ☕ au lait', 'café ☕ au lait 🥛', 'Café ☕ au lait')
    expect(r.text).toBe('Café ☕ au lait 🥛')
    expect(r.conflict).toBe(false)
  })

  test('pathological input degrades to a whole-text edit, not a hang', () => {
    const gen = (p: string): string => Array.from({ length: 6_000 }, (_, i) => `${p}${i} `).join('')
    const [a, b, c] = [gen('a'), gen('b'), gen('c')]
    const r = merge3(a, b, c)
    expect(r.conflict).toBe(true)
    expect(r.text).toBe(c)
  })

  test('tokenizer round trips', () => {
    for (const s of ['', 'a', 'hello  world\n', 'x_1+y', '  \t ', '日本語 text ☕☕']) {
      expect(tokenize(s).join('')).toBe(s)
    }
    expect(tokenize('ab  c\n\nd')).toEqual(['ab', '  ', 'c', '\n', '\n', 'd'])
  })

  test('diff hunks apply back', () => {
    const a = tokenize('the quick brown fox jumps')
    const b = tokenize('the slow brown dog jumps over')
    let out = ''
    let pos = 0
    for (const h of diff(a, b)) {
      out += a.slice(pos, h.aStart).join('')
      out += b.slice(h.bStart, h.bEnd).join('')
      pos = h.aEnd
    }
    out += a.slice(pos).join('')
    expect(out).toBe(b.join(''))
  })
})

// ── property-style tests: the same deterministic xorshift generator as the Rust tests ──

class Rng {
  constructor(private s: bigint) {}
  next(): bigint {
    const mask = 0xffff_ffff_ffff_ffffn
    this.s ^= (this.s << 13n) & mask
    this.s ^= this.s >> 7n
    this.s ^= (this.s << 17n) & mask
    return this.s
  }
  below(n: number): number {
    return Number(this.next() % BigInt(n))
  }
}
const WORDS = ['milk', 'eggs', 'Δelta', 'x', 'bread', 'tea', '☕', 'ok', 'a_b', '42']
const word = (r: Rng): string => WORDS[r.below(WORDS.length)]
const line = (r: Rng): string => {
  const n = r.below(4) + 1
  return Array.from({ length: n }, () => word(r)).join(' ')
}
const doc = (r: Rng, n: number): string[] => Array.from({ length: n }, () => line(r))
const join = (lines: string[]): string => lines.join('\n\n')

describe('merge3 properties (port of the merge.rs property tests)', () => {
  test('edits to disjoint lines always compose and commute', () => {
    const r = new Rng(0x9e37_79b9_7f4a_7c15n)
    for (let iter = 0; iter < 300; iter++) {
      const n = 3 + r.below(8)
      const base = doc(r, n)
      const theirs = [...base]
      const mine = [...base]
      const expected = [...base]
      for (let i = 0; i < n; i++) {
        const pick = r.below(4)
        if (pick === 0) {
          theirs[i] = `${base[i]} T`
          expected[i] = theirs[i]
        } else if (pick === 1) {
          mine[i] = `M ${base[i]}`
          expected[i] = mine[i]
        }
      }
      const [b, t, m] = [join(base), join(theirs), join(mine)]
      const r1 = merge3(b, t, m)
      expect(r1.conflict).toBe(false)
      expect(r1.text).toBe(join(expected))
      expect(merge3(b, m, t).text).toBe(r1.text) // composition is symmetric
    }
  })

  test('overlapping edits resolve to mine and keep theirs as lost', () => {
    const r = new Rng(0xdead_beef_cafe_f00dn)
    for (let iter = 0; iter < 300; iter++) {
      const n = 2 + r.below(6)
      const base = doc(r, n)
      const i = r.below(n)
      const theirs = [...base]
      const mine = [...base]
      const rewrite = (tag: string): string =>
        base[i]
          .split(' ')
          .map((_, k) => `${tag}${k}x`)
          .join(' ')
      theirs[i] = rewrite('t')
      mine[i] = rewrite('m')
      const [b, t, m] = [join(base), join(theirs), join(mine)]
      const res = merge3(b, t, m)
      expect(res.conflict).toBe(true)
      expect(res.text).toBe(m)
      expect(res.lost).toBe(t)
    }
  })

  test('identities and robustness on random text', () => {
    const r = new Rng(0x1234_5678_9abc_def1n)
    const noisy = (from: string): string => {
      const words = from.split(' ')
      const damage = r.below(4)
      for (let k = 0; k < damage; k++) {
        const at = r.below(words.length + 1)
        const op = r.below(3)
        if (op === 0 && at < words.length) words.splice(at, 1)
        else if (op === 1) words.splice(at, 0, word(r))
        else if (at < words.length) words[at] = word(r)
      }
      return words.join(' ')
    }
    for (let iter = 0; iter < 500; iter++) {
      const base = `${line(r)} ${line(r)} ${line(r)}`
      const theirs = noisy(base)
      const mine = noisy(base)
      expect(merge3(base, base, base).text).toBe(base)
      expect(merge3(base, theirs, base).text).toBe(theirs)
      expect(merge3(base, base, mine).text).toBe(mine)
      expect(merge3(base, theirs, theirs).text).toBe(theirs)
      const res = merge3(base, theirs, mine)
      expect(res.conflict).toBe(res.lost !== null)
      if (!res.conflict) expect(merge3(base, mine, theirs).conflict).toBe(false) // conflict-ness is symmetric
    }
  })
})

describe('diff1', () => {
  test('is the single edit between two strings', () => {
    expect(diff1('hello world', 'hello brave world')).toEqual({ at: 6, del: 0, ins: 'brave ' })
    expect(diff1('abcdef', 'abXef')).toEqual({ at: 2, del: 2, ins: 'X' })
    expect(diff1('same', 'same')).toEqual({ at: 4, del: 0, ins: '' })
  })
})

describe('mapCaret', () => {
  test('a caret before the change stays; after it shifts by the length difference', () => {
    expect(mapCaret('hello world', 'hello brave world', 2)).toBe(2)
    expect(mapCaret('hello world', 'hello brave world', 9)).toBe(15)
  })
  test('a caret inside a replaced span lands at the end of the replacement', () => {
    expect(mapCaret('abcdef', 'abXYef', 3)).toBe(4)
  })
  test('a caret at the end of an appended value stays at the end of what it was', () => {
    expect(mapCaret('milk', 'milk\n- oat', 4)).toBe(4)
  })
})

describe('rebase', () => {
  test('keystrokes typed while a write was in flight survive a merged ack', () => {
    // sent "Milk", the host merged an agent line, the user then typed "!" at the start of the draft.
    expect(rebase('Milk', 'Milk\n- oat', '!Milk')).toBe('!Milk\n- oat')
  })
  test('an ack equal to what was sent leaves the draft alone', () => {
    expect(rebase('a', 'a', 'ab')).toBe('ab')
  })
  test('the draft and the ack edit different words of the sent text', () => {
    expect(rebase('buy milk and eggs', 'buy milk and eggs\n- tea', 'buy oat milk and eggs')).toBe(
      'buy oat milk and eggs\n- tea'
    )
  })
})
