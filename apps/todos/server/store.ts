// The lists and their todos, in the app's own database. Everything that changes them is here, and says why when it
// cannot: a function that changes something answers with a plain sentence when it refused (the handler and the tool
// pass it on as it is), and with nothing when it did what it was asked.
import { db } from 'vos:app'
import { Pattern } from 'lib:regex'
import { trim } from 'lib:text'
import { caseless_eq } from 'lib:unicode'
import type { Board, Color, List, Todo } from '../.app/server/rpc'

/** What can change on a todo: only what is given changes. */
export interface Changes {
  title?: string
  notes?: string
  due?: string
  flagged?: boolean
}

// A row of the database is whatever the statement selected: the declarations say `unknown`, and each statement's
// reader says what its columns are.
type Row = { [column: string]: unknown }

// completed todos are kept, but a board carries only the most recent of them
const COMPLETED_SHOWN = 500

const TODO = 'select id, list_id, title, notes, due, flagged, done from todos'

db.migrate([
  // The text starts on the backtick's own line: the database keeps a checksum of a migration's bytes, and these are
  // the bytes every existing Todos database was made from. A line break after the backtick would be one more.
  `    create table lists (
      id integer primary key,
      name text not null,
      color text not null
    );
    create table todos (
      id integer primary key,
      list_id integer not null,
      title text not null,
      notes text not null default '',
      due text not null default '',
      flagged integer not null default 0,
      done integer not null default 0,
      completed_at text
    );
    create index todos_by_list on todos(list_id, id);
    create index todos_by_completion on todos(done, completed_at desc, id desc);
    insert into lists(name, color) values ('Reminders', 'blue');
  `
])

// `[0-9]`, not `\\d`: the host's patterns are Unicode ones, where `\\d` is a digit of any script
const DAY = pattern('^[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])$')

/** A pattern of the app's own: one that does not compile is a mistake here, found when the app loads. */
function pattern(source: string): Pattern {
  const [compiled, problem] = Pattern.new(source)
  if (compiled === undefined) throw new Error(`the pattern ${source} does not compile: ${problem?.message}`)
  return compiled
}

/** A day is `2026-06-09`; empty is no day. */
function isDay(text: string): boolean {
  return text === '' || DAY.is_match(text)
}

function todoOf(row: Row): Todo {
  return {
    id: row.id as number,
    list: row.list_id as number,
    title: row.title as string,
    notes: row.notes as string,
    due: row.due as string,
    flagged: row.flagged === 1,
    done: row.done === 1
  }
}

function listOf(row: Row): List {
  return { id: row.id as number, name: row.name as string, color: row.color as Color }
}

export function lists(): List[] {
  return db.all('select id, name, color from lists order by id').map(listOf)
}

/** The list called `name`, whatever its capitals: how an agent names a list. */
export function listNamed(name: string): List | undefined {
  const wanted = trim(name)
  return lists().find((list) => caseless_eq(list.name, wanted))
}

export function todo(id: number): Todo | undefined {
  const row = db.one(`${TODO} where id = :id`, { id })
  return row === undefined ? undefined : todoOf(row)
}

/** The open todos in the order they were added. */
export function open(): Todo[] {
  return db.all(`${TODO} where done = 0 order by id`).map(todoOf)
}

/** The completed todos, the most recently completed first. */
export function completed(): Todo[] {
  return db.all(`${TODO} where done = 1 order by completed_at desc, id desc limit :n`, { n: COMPLETED_SHOWN }).map(todoOf)
}

export function board(): Board {
  return { lists: lists(), todos: [...open(), ...completed()] }
}

export function addList(name: string, color: Color): string | undefined {
  const named = trim(name)
  if (named === '') return 'A list needs a name.'
  db.exec('insert into lists(name, color) values (:n, :c)', { n: named, c: color })
  return undefined
}

export function removeList(id: number): string | undefined {
  if (db.one('select id from lists where id = :id', { id }) === undefined) return 'There is no such list.'
  const count = db.one('select count(*) as n from lists')
  if (count === undefined || (count.n as number) <= 1) return 'The last list cannot be deleted.'
  db.tx(() => {
    db.exec('delete from todos where list_id = :id', { id })
    db.exec('delete from lists where id = :id', { id })
  })
  return undefined
}

/** Add a todo: answers with its id, or with why not. */
export function add(list: number, title: string, given: Changes = {}): { id: number } | { problem: string } {
  const titled = trim(title)
  const due = given.due ?? ''
  if (titled === '') return { problem: 'A todo needs a title.' }
  if (!isDay(due)) return { problem: 'A day is written like 2026-06-09.' }
  if (db.one('select id from lists where id = :id', { id: list }) === undefined) return { problem: 'There is no such list.' }
  const made = db.exec('insert into todos(list_id, title, notes, due, flagged) values (:l, :t, :n, :d, :f)', {
    l: list,
    t: titled,
    n: given.notes ?? '',
    d: due,
    f: given.flagged ? 1 : 0
  })
  return { id: made.last_insert_id }
}

export function update(id: number, changes: Changes): string | undefined {
  const found = todo(id)
  if (found === undefined) return 'There is no such todo.'
  const title = changes.title !== undefined ? trim(changes.title) : found.title
  const due = changes.due ?? found.due
  if (title === '') return 'A todo needs a title.'
  if (!isDay(due)) return 'A day is written like 2026-06-09.'
  db.exec('update todos set title = :t, notes = :n, due = :d, flagged = :f where id = :id', {
    id,
    t: title,
    n: changes.notes ?? found.notes,
    d: due,
    f: (changes.flagged ?? found.flagged) ? 1 : 0
  })
  return undefined
}

/** Complete a todo (`done`), or bring it back. */
export function complete(id: number, done: boolean): string | undefined {
  const changed = db.exec(
    `update todos
      set done = :d,
          completed_at = case when :d = 1 then strftime('%Y-%m-%dT%H:%M:%fZ', 'now') else null end
      where id = :id`,
    { id, d: done ? 1 : 0 }
  )
  return changed.changes === 0 ? 'There is no such todo.' : undefined
}

export function remove(id: number): string | undefined {
  const changed = db.exec('delete from todos where id = :id', { id })
  return changed.changes === 0 ? 'There is no such todo.' : undefined
}
