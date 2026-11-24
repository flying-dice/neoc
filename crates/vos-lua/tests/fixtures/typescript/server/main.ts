// A backend in TypeScript that tries what the compiler's output needs from an app's environment: its runtime library
// (classes, Map and Set, generators, string and array methods, Error), the bitwise operators, the host's modules and
// the `app` API, called as methods and as plain functions. `crates/vos-lua/tests/typescript.rs` builds it with the
// product's own pipeline and runs it in the host's runtime.
import { db } from 'vos:app'
import * as base64 from 'lib:base64'
import { Pattern } from 'lib:regex'
import { trim } from 'lib:text'
import type { Handlers } from '../.app/server/rpc'
import { Tally, upTo } from './shapes'

class Refused extends Error {}

db.migrate(['create table words (id integer primary key, word text not null)'])

const handlers: Handlers = {
  probe: () => {
    const lines: string[] = []
    const say = (name: string, value: unknown): void => void lines.push(`${name}=${value}`)

    // the language and its runtime library
    const tally = new Tally(3)
    tally.next()
    say('class', tally.next())
    say('getter', tally.double)
    const ages = new Map<string, number>([['ada', 36]])
    ages.set('alan', 41)
    say('map', [...ages.keys()].join(','))
    say('set', new Set([1, 2, 2, 3]).size)
    say('generator', [...upTo(4)].join(''))
    say('array', [3, 1, 2].sort((a, b) => a - b).map((n) => n * 2).filter((n) => n > 2).join('+'))
    say('string', 'a-b-c'.split('-').map((part) => part.toUpperCase()).join('').padStart(5, '.'))
    say('number', `${Math.floor(7 / 2)} ${(3.14159).toFixed(2)} ${parseInt('42px')} ${Number('x') !== Number('x')}`)
    const loose: { a?: number; b?: { c: string } } = { a: 1 }
    say('optional', `${loose.b?.c ?? 'none'} ${loose.a ?? 0}`)
    const { a, ...others } = { a: 1, b: 2, c: 3 }
    say('spread', `${a} ${Object.keys(others).sort().join('')}`)

    // an Error is thrown, caught and read without the `debug` library
    try {
      throw new Refused('not this one')
    } catch (error) {
      say('caught', error instanceof Refused && error instanceof Error ? error.message : 'something else')
      say('shown', trim(String(error)))
    }

    // the bitwise operators are Luau's bit32: unsigned, so `>>` of a negative number keeps the sign bit as a large number
    say('bits', [6 & 3, 6 | 1, 6 ^ 3, 1 << 4, 256 >> 2, 256 >>> 4, ~0xfffffff0, -8 >> 1, -8 >>> 1].join(' '))

    // the host's modules: a plain function, several values back, a method on what one returned
    say('module', base64.encode(trim('  hi  ')))
    const [decoded, problem] = base64.decode('aGk=')
    say('values', `${decoded} ${problem === undefined}`)
    const [pattern] = Pattern.new('^h')
    say('method', pattern?.is_match('hi'))
    return lines
  },

  raise: (ctx, params) => {
    switch (params.how) {
      case 'error':
        throw new Error('a plain error')
      case 'subclass':
        throw new Refused('a refusal')
      case 'coded':
        throw 'RPC-32001 not today'
      case 'fail':
        return ctx.fail('Not like that.')
    }
  },

  remember: (_ctx, params) => {
    db.tx(() => db.exec('insert into words(word) values (:w)', { w: params.word }))
    return db.all('select word from words order by id').map((row) => row.word as string)
  }
}

export default handlers
