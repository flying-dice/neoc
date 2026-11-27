/**
 * Type-level contract of `types/generated/vos.d.ts`: an app's backend written in TypeScript, as a developer writes it
 * (`bun run typecheck` runs `tsc -p types/backend`, with the declarations themselves checked: no `skipLibCheck`). The
 * `@ts-expect-error` lines prove that what an app is not offered is rejected: if the declarations stop refusing it, the
 * directive is unused and tsc fails. What TypeScriptToLua makes of calls like these (`:` or `.`) is held by
 * `packages/vite-plugin/src/typescript-backend.test.ts`.
 */
import * as vos from 'vos:app'
import { db, type Actor, type Context, type FileError } from 'vos:app'
import * as rpc from 'vos:rpc'
import * as fs from 'std:fs'
import * as env from 'std:env'
import { trim } from 'lib:text'
import { Duration, type DateTime } from 'lib:chrono'
import * as json from 'lib:json'
import * as collections from 'std:collections'

// a module's functions are plain functions; a value's are methods
const name: string = trim('  a  ')
const [text, problem] = fs.read('notes.txt')
// a fallible call returns the value or a typed error: checking one narrows the other
const length: number = problem === undefined ? text.length : problem.kind === 'not_found' ? 0 : problem.message.length
const counter: std_collections_Counter = collections.Counter.new(1)
const next: number = counter.increment()
const [half, unparsed]: [lib_chrono_Duration, undefined] | [undefined, lib_chrono_ParseError] = Duration.parse('30s')
declare const when: DateTime
const same: lib_chrono_DateTime = when

// a member whose name is a word of TypeScript is still the module's member
const [home] = env.var('HOME')
const nothing: lib_json_Object = json.object()

// the app API: the module, its named exports and the global are one thing
db.migrate(['create table notes (id integer primary key, title text not null)'])
const made: vos_app_ExecResult = db.exec('insert into notes(title) values (:t)', { t: name })
const row: { [key: string]: unknown } | undefined = vos.db.one('select * from notes where id = :id', { id: made.last_insert_id })
// what a transaction's body returns is the app's own: the declarations say `unknown`, the app says what it is
const total = app.db.tx(() => app.db.all('select * from notes').length) as number
const stored: unknown = app.kv.get('seen')
app.kv.delete('seen')
vos.notify({ title: 'Saved' })
vos.open_window()
vos.open_window({ note: 1 })
const id: string = vos.id
rpc.send_notification('notes.changed', { id: 1 })

rpc.on_request(
  'count_notes',
  // the declarations cannot know the shape of the params: the handler says it (the contract holds them to it)
  (params, ctx) => (ctx.actor.kind === 'agent' ? { n: total, folder: (params as { folder?: string }).folder } : ctx.fail('Agents only.'))
)

// a private method (`_`) is the app's page's alone
rpc.on_request('_who', (_params, ctx: Context): Actor => ctx.actor)
rpc.on_request('_fail', (_params, ctx) => ctx.fail('No.'))

// a failed file call returns a typed error: checking it narrows the value, and its kind is one of a declared set
const [data, failed] = app.files.documents.read('notes/a.txt')
const size: number = failed === undefined ? data.length : failed.kind === 'not_found' ? 0 : -1
const why: FileError | undefined = failed
// @ts-expect-error an error's kind is one of the declared kinds
const misspelt: boolean = failed?.kind === 'not_fond'

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

export { half, unparsed, home, id, length, misspelt, next, nothing, refused, row, same, size, stored, whole, why, workers }
