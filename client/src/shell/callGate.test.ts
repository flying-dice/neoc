import { describe, expect, test } from 'bun:test'
import { createCallGate } from './callGate'

const tick = (): Promise<void> => new Promise((r) => setTimeout(r, 0))

/** Run `count` calls of one frame through the gate; each takes `ms` and the gate's peak concurrency is recorded. */
async function burst(
  gate: ReturnType<typeof createCallGate>,
  frame: object,
  count: number,
  meter: Meter,
  ms = 1
): Promise<(string | null)[]> {
  return Promise.all(
    Array.from({ length: count }, async (_, i) => {
      const release = await gate.enter(frame)
      if (release === null) return 'refused'
      meter.now += 1
      meter.frames.set(frame, (meter.frames.get(frame) ?? 0) + 1)
      meter.peak = Math.max(meter.peak, meter.now)
      await new Promise((r) => setTimeout(r, ms))
      meter.now -= 1
      meter.frames.set(frame, (meter.frames.get(frame) ?? 1) - 1)
      meter.done += 1
      release()
      return `${i}`
    })
  )
}

interface Meter {
  now: number
  peak: number
  done: number
  frames: Map<object, number>
}
const newMeter = (): Meter => ({ now: 0, peak: 0, done: 0, frames: new Map<object, number>() })

describe('the call gate', () => {
  test('a burst of fifty calls from one frame completes, with a few in flight at a time', async () => {
    const gate = createCallGate({ total: 32, perFrame: 8, maxWaiting: 1000 })
    const meter = newMeter()
    const answers = await burst(gate, {}, 50, meter)
    expect(answers.every((a) => a !== 'refused')).toBe(true)
    expect(meter.done).toBe(50)
    expect(meter.peak).toBeLessThanOrEqual(8)
  })

  test('five thousand parallel calls all get their turn, none are refused', async () => {
    const gate = createCallGate({ total: 32, perFrame: 8, maxWaiting: 5000 })
    const meter = newMeter()
    const answers = await burst(gate, {}, 5000, meter, 0)
    expect(answers.filter((a) => a === 'refused').length).toBe(0)
    expect(meter.done).toBe(5000)
    expect(meter.peak).toBeLessThanOrEqual(8)
  })

  test('all frames together never exceed the total, so the shell’s own calls keep their slots', async () => {
    const gate = createCallGate({ total: 12, perFrame: 8, maxWaiting: 1000 })
    const meter = newMeter()
    await Promise.all([1, 2, 3, 4].map((n) => burst(gate, { n }, 100, meter)))
    expect(meter.done).toBe(400)
    expect(meter.peak).toBeLessThanOrEqual(12)
  })

  test('frames take turns: a small call of one frame is not stuck behind another frame’s burst', async () => {
    const gate = createCallGate({ total: 4, perFrame: 4, maxWaiting: 1000 })
    const big = {}
    const small = {}
    const order: string[] = []
    const run = async (frame: object, name: string, ms: number): Promise<void> => {
      const release = (await gate.enter(frame)) as () => void
      order.push(name)
      await new Promise((r) => setTimeout(r, ms))
      release()
    }
    const bigCalls = Array.from({ length: 40 }, (_, i) => run(big, `big${i}`, 5))
    await tick()
    const smallCall = run(small, 'small', 1)
    await Promise.all([...bigCalls, smallCall])
    expect(order.indexOf('small')).toBeLessThan(20) // served after the first round of big ones, not after all forty
  })

  test('a frame with too many waiting is refused with null, and the others are unaffected', async () => {
    const gate = createCallGate({ total: 1, perFrame: 1, maxWaiting: 2 })
    const greedy = {}
    const first = (await gate.enter(greedy)) as () => void
    const waiting = [gate.enter(greedy), gate.enter(greedy)]
    expect(await gate.enter(greedy)).toBeNull()
    first()
    const next = (await waiting[0]) as () => void
    next()
    ;((await waiting[1]) as () => void)()
    const other = await gate.enter({})
    expect(other).not.toBeNull()
    other?.()
  })

  test('a place given back twice is given back once', async () => {
    const gate = createCallGate({ total: 2, perFrame: 2, maxWaiting: 10 })
    const a = {}
    const r1 = (await gate.enter(a)) as () => void
    const r2 = (await gate.enter(a)) as () => void
    const third = gate.enter(a)
    r1()
    const r3 = (await third) as () => void
    r2()
    r2()
    r3()
    r3()
    // two places taken now, a third waits: had the second give-back counted, three would run
    const x = (await gate.enter(a)) as () => void
    const y = (await gate.enter(a)) as () => void
    let z = false
    void gate.enter(a).then(() => (z = true))
    await tick()
    expect(z).toBe(false)
    x()
    y()
  })
})
