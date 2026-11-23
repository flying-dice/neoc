/**
 * Type-level contract of `types/generated/vos.d.ts`: an app's backend written in TypeScript, as a developer writes it
 * (`bun run typecheck` runs `tsc -p types/backend`, with the declarations themselves checked: no `skipLibCheck`). The
 * `@ts-expect-error` lines prove that what an app is not offered is rejected: if the declarations stop refusing it, the
 * directive is unused and tsc fails. What TypeScriptToLua makes of calls like these (`:` or `.`) is held by
 * `packages/vite-plugin/src/typescript-backend.test.ts`.
 */
import * as vos from 'vos:app'
import { db, t, type Actor, type HandlerContext } from 'vos:app'
import * as fs from 'std:fs'
import * as env from 'std:env'
import { trim } from 'lib:text'
import { Duration, type DateTime } from 'lib:chrono'
import * as json from 'lib:json'
import * as collections from 'std:collections'

// a module's functions are plain functions; a value's are methods
const name: string = trim('  a  ')
const [text, problem] = fs.read('notes.txt')
const length: number = text === undefined ? (problem ?? '').length : text.length
const counter: std_collections_Counter = collections.Counter.new(1)
const next: number = counter.increment()
const [half]: [lib_chrono_Duration | undefined, string | undefined] = Duration.parse('30s')
declare const when: DateTime
const same: lib_chrono_DateTime = when

// a member whose name is a word of TypeScript is still the module's member
const [home] = env.var('HOME')
const nothing: lib_json_JsonNull = json.null

// the app API: the module, its named exports and the global are one thing
db.migrate({ [1]: 'create table notes (id integer primary key, title text not null)' })
const made: vos_ExecResult = db.exec('insert into notes(title) values (:t)', { t: name })
const row: { [key: string]: any } | undefined = vos.db.one('select * from notes where id = :id', { id: made.last_insert_id })
const total: number = app.db.tx(() => app.db.all('select * from notes').length)
const stored: any = app.kv.get('seen')
app.kv.delete('seen')
vos.notify({ title: 'Saved' })
vos.openWindow()
vos.emit('notes.changed', { id: 1 })

vos.tool({
  name: 'count_notes',
  readOnly: true,
  input: { folder: t.string({ optional: true, description: 'Only this folder' }) },
  run: (input: { folder?: string }, ctx) => (ctx.actor.kind === 'agent' ? { n: total } : ctx.fail('Agents only.'))
})

const handlers: vos_Handlers = {
  who: (ctx: HandlerContext): Actor => ctx.actor,
  fail: (ctx) => ctx.fail('No.')
}

// a notification has a title and a body, and nothing else
// @ts-expect-error a notification is not scheduled
vos.notify({ title: 'Later', at: '2026-06-09' })
// @ts-expect-error a result of several values is not one value
const whole: string = fs.read('notes.txt')

// what an app is not offered is not declared
// @ts-expect-error lib:test is not offered to apps
import * as refused from 'lib:test'
// @ts-expect-error std:workers is not offered to apps
import * as workers from 'std:workers'
// @ts-expect-error an app has no `print`
print('x')
// @ts-expect-error an app has no `os`
os.time()

export { handlers, half, home, length, next, nothing, refused, row, same, stored, whole, workers }
