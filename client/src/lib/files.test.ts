/* eslint-disable @typescript-eslint/explicit-function-return-type -- test helpers infer their return types */
import { describe, expect, test } from 'bun:test'
import { FileBrowser, folderNameProblem, freeName, parseHostPath } from './files.svelte'
import { HostError } from '../shell/channel'
import {
  createFakeHost,
  FAKE_AGENTS_APP,
  FAKE_CALCULATOR,
  type FakeFolder
} from '../shell/fakeHost'
import { fileSize, fileUrl, type FilePlace, type HostApi } from '../shell/hostApi'
import type { Uploader } from '../shell/upload'

const documents = {
  agents: {
    transcripts: { 'b.md': 20, 'A.md': 10, nested: { 'deep.txt': 1 } },
    'report.csv': 1500,
    'Plan.md': 12
  }
}
const computer = {
  home: { me: { Documents: { 'letter.txt': 9 }, 'todo.md': 3 } },
  locked: 'locked: You do not have permission to open this folder.',
  tmp: {}
}
const HOME = ['home', 'me']
const AGENTS: FilePlace = { kind: 'app', app: FAKE_AGENTS_APP }
const CALCULATOR: FilePlace = { kind: 'app', app: FAKE_CALCULATOR }
const COMPUTER: FilePlace = { kind: 'computer' }
const never: Uploader = async () => ({ status: 0, message: 'no uploads in this test' })
const silent = { onNotification: () => () => {} }
const fileOf = (name: string, text = 'x') => ({ name, size: text.length, blob: new Blob([text]) })
/** The home directory of a fake host's file system. */
const homeOf = (fake: { computer: FakeFolder | null }) =>
  (fake.computer!.home as FakeFolder).me as FakeFolder

function setup(options: Parameters<typeof createFakeHost>[0] = {}) {
  const fake = createFakeHost({
    apps: [FAKE_CALCULATOR, FAKE_AGENTS_APP],
    files: documents,
    computer,
    home: '/home/me',
    ...options
  })
  const calls: [string, Record<string, unknown>][] = []
  const sent: string[] = []
  const api: HostApi = {
    call: (method, params = {}) => {
      calls.push([method, params])
      return fake.call(method, params)
    },
    onNotification: fake.onNotification
  }
  const send: Uploader = (url, file, onProgress) => {
    sent.push(url)
    return fake.upload(url, file, onProgress)
  }
  return { fake, calls, sent, browser: new FileBrowser(api, send) }
}

const names = (browser: FileBrowser) => browser.visibleEntries.map((e) => e.name)

describe('the places', () => {
  test('are Home and This Computer, then every installed app, those that keep documents first', async () => {
    const { browser, fake } = setup()
    await browser.setApps(fake.apps)
    expect([browser.computer, browser.showsHome, browser.showsComputer]).toEqual([true, true, true])
    expect(browser.home).toEqual(HOME)
    expect(browser.maxUploadBytes).toBe(64 * 1024 * 1024)
    expect(browser.folders.map((f) => [f.app.id, f.count])).toEqual([
      ['agents', 3],
      ['calculator', 0]
    ])
    expect([browser.loading, browser.error, browser.title]).toEqual([false, null, 'Files'])
  })

  test('a host with its file system turned off offers only the apps', async () => {
    const { browser, fake } = setup()
    fake.computer = null
    await browser.setApps(fake.apps)
    expect([browser.computer, browser.showsHome, browser.showsComputer]).toEqual([
      false,
      false,
      false
    ])
    expect(browser.folders.length).toBe(2)
  })

  test('the search filters the places by name', async () => {
    const { browser, fake } = setup()
    await browser.setApps(fake.apps)
    browser.query = ' CALC '
    expect(browser.visibleFolders.map((f) => f.app.id)).toEqual(['calculator'])
    expect([browser.showsHome, browser.showsComputer]).toEqual([false, false])
    browser.query = 'comp'
    expect([browser.showsHome, browser.showsComputer]).toEqual([false, true])
    expect(browser.visibleFolders).toEqual([])
  })

  test('a host that cannot read anything says why; one app that cannot be read does not hide the others', async () => {
    const broken = setup({ filesError: 'the disk is not there' })
    await broken.browser.setApps(broken.fake.apps)
    expect(broken.browser.error).toBe('the disk is not there')

    const { browser, fake } = setup()
    // an app that was uninstalled between the list of apps and the read of its files
    await browser.setApps([...fake.apps, { ...FAKE_CALCULATOR, id: 'gone', name: 'Gone' }])
    expect(browser.error).toBeNull()
    expect(browser.folders.map((f) => [f.app.id, f.count])).toEqual([
      ['agents', 3],
      ['calculator', 0],
      ['gone', null]
    ])
  })

  test('a host that answers in another shape is an error, not a crash', async () => {
    const odd = new FileBrowser({ call: async () => ({ entries: null }), ...silent }, never)
    await odd.setApps([FAKE_CALCULATOR])
    expect(odd.error).toContain('unexpected answer')
  })
})

describe('browsing an app', () => {
  test('opens its Documents, folders first then by name, and goes into a folder and back', async () => {
    const { browser, fake, calls } = setup()
    await browser.setApps(fake.apps)
    await browser.open(AGENTS)
    expect(calls.at(-1)).toEqual(['host.files.list', { app: 'agents', path: '' }])
    expect(names(browser)).toEqual(['transcripts', 'Plan.md', 'report.csv'])
    expect([browser.title, browser.parentTitle, browser.pathText]).toEqual(['Agents', 'Files', ''])

    await browser.enter('transcripts')
    expect(calls.at(-1)).toEqual(['host.files.list', { app: 'agents', path: 'transcripts' }])
    expect(names(browser)).toEqual(['nested', 'A.md', 'b.md'])
    expect([browser.title, browser.parentTitle]).toEqual(['transcripts', 'Agents'])

    await browser.enter('nested')
    expect(calls.at(-1)?.[1]).toEqual({ app: 'agents', path: 'transcripts/nested' })
    expect([browser.title, browser.parentTitle]).toEqual(['nested', 'transcripts'])

    await browser.back()
    expect(browser.path).toEqual(['transcripts'])
    await browser.back()
    expect(names(browser)).toEqual(['transcripts', 'Plan.md', 'report.csv'])
    await browser.back()
    expect(browser.place).toBeNull()
    expect(browser.folders.length).toBe(2)
  })

  test('an app with no documents opens on an empty folder', async () => {
    const { browser, fake } = setup()
    await browser.setApps(fake.apps)
    await browser.open(CALCULATOR)
    expect([browser.entries, browser.error, browser.loading]).toEqual([[], null, false])
  })

  test('the search filters the open folder by name and is cleared by moving', async () => {
    const { browser, fake } = setup()
    await browser.setApps(fake.apps)
    await browser.open(AGENTS)
    browser.query = 'PLAN'
    expect(names(browser)).toEqual(['Plan.md'])
    browser.query = 'nothing like it'
    expect(names(browser)).toEqual([])
    await browser.enter('transcripts')
    expect(browser.query).toBe('')
  })

  test('a folder that is gone is an error with nothing listed, and reading again recovers', async () => {
    const { browser, fake } = setup()
    await browser.setApps(fake.apps)
    await browser.open(AGENTS)
    await browser.enter('drafts')
    expect(browser.error).toContain('no such file or directory')
    expect(browser.entries).toEqual([])
    fake.files.agents.drafts = { 'a.txt': 1 }
    await browser.refresh()
    expect(browser.error).toBeNull()
    expect(names(browser)).toEqual(['a.txt'])
  })

  test('an answer for a folder the person already left is dropped', async () => {
    const answers: ((entries: unknown) => void)[] = []
    const api: HostApi = {
      call: () =>
        new Promise((resolve) => answers.push((entries) => resolve({ entries, truncated: false }))),
      ...silent
    }
    const browser = new FileBrowser(api, never)
    const opening = browser.open(AGENTS)
    const entering = browser.enter('transcripts')
    const file = (name: string) => ({ name, kind: 'file', size: 1, link: false })
    answers[1]([file('inside.md')])
    await entering
    answers[0]([file('top.md')]) // the first read answers late
    await opening
    expect(browser.path).toEqual(['transcripts'])
    expect(names(browser)).toEqual(['inside.md'])
    expect(browser.loading).toBe(false)
  })

  test('an app that is uninstalled while its files are open goes back to the places', async () => {
    const { browser, fake } = setup()
    await browser.setApps(fake.apps)
    await browser.open(AGENTS)
    await browser.setApps([FAKE_CALCULATOR])
    expect(browser.place).toBeNull()
    expect(browser.folders.map((f) => f.app.id)).toEqual(['calculator'])
  })

  test('a file downloads from its path under the place, each name encoded', async () => {
    const { browser, fake } = setup()
    await browser.setApps(fake.apps)
    await browser.open(AGENTS)
    await browser.enter('transcripts')
    expect(browser.downloadUrl(browser.entries[0])).toBe('/files/app/agents/transcripts/A.md')
    expect(fileUrl(AGENTS, ['été "1"', 'a/b?.txt'])).toBe(
      '/files/app/agents/%C3%A9t%C3%A9%20%221%22/a%2Fb%3F.txt'
    )
    expect(fileUrl(COMPUTER, ['etc', 'hosts'])).toBe('/files/host/etc/hosts')
  })
})

describe('This Computer', () => {
  test('is browsed by absolute path with the same calls and no app', async () => {
    const { browser, fake, calls } = setup()
    await browser.setApps(fake.apps)
    await browser.open(COMPUTER, HOME)
    expect(calls.at(-1)).toEqual(['host.files.list', { path: '/home/me' }])
    expect(names(browser)).toEqual(['Documents', 'todo.md'])
    expect([browser.title, browser.parentTitle, browser.pathText]).toEqual([
      'me',
      'home',
      '/home/me'
    ])
    await browser.enter('Documents')
    expect(calls.at(-1)).toEqual(['host.files.list', { path: '/home/me/Documents' }])
    expect(browser.downloadUrl(browser.entries[0])).toBe('/files/host/home/me/Documents/letter.txt')
    await browser.remove(browser.entries[0])
    expect(calls.at(-2)).toEqual(['host.files.remove', { path: '/home/me/Documents/letter.txt' }])
    expect(names(browser)).toEqual([])
    // an uninstall elsewhere does not close This Computer
    await browser.setApps([])
    expect(browser.place).toEqual(COMPUTER)
  })

  test('Home is a shortcut into it, and Back leads up to the top of the disk and then out', async () => {
    const { browser, fake } = setup()
    await browser.setApps(fake.apps)
    await browser.open(COMPUTER, browser.home!)
    await browser.back()
    expect([browser.pathText, names(browser)]).toEqual(['/home', ['me']])
    await browser.back()
    expect([browser.pathText, browser.title, browser.parentTitle]).toEqual([
      '/',
      'This Computer',
      'Files'
    ])
    expect(names(browser)).toEqual(['home', 'locked', 'tmp'])
    await browser.back()
    expect(browser.place).toBeNull()
  })

  test('a path that is typed or pasted is gone to; one that is not absolute is said and not sent', async () => {
    const { browser, fake, calls } = setup()
    await browser.setApps(fake.apps)
    await browser.goTo(' /home/me/../me/Documents/ ')
    expect(browser.place).toEqual(COMPUTER)
    expect(browser.pathText).toBe('/home/me/Documents')
    expect(names(browser)).toEqual(['letter.txt'])
    const before = calls.length
    await browser.goTo('Documents')
    expect(browser.pathProblem).toContain('starts with “/”')
    expect(calls.length).toBe(before)
    expect(browser.pathText).toBe('/home/me/Documents')
    await browser.goTo('/')
    expect([browser.pathProblem, browser.pathText]).toEqual([null, '/'])
    expect(parseHostPath('/a/./b/../c')).toEqual(['a', 'c'])
    expect(parseHostPath('~/x')).toBeNull()
  })

  test('a folder the host may not open says so, and Back still works', async () => {
    const { browser, fake } = setup()
    await browser.setApps(fake.apps)
    await browser.goTo('/')
    await browser.enter('locked')
    expect(browser.error).toBe('locked: You do not have permission to open this folder.')
    expect(browser.entries).toEqual([])
    await browser.back()
    expect(browser.error).toBeNull()
    expect(names(browser)).toEqual(['home', 'locked', 'tmp'])
  })

  test('a folder with more entries than a listing carries says there are more', async () => {
    const entries = [{ name: 'a', kind: 'file', size: 1, link: false }]
    const capped = new FileBrowser(
      { call: async () => ({ entries, truncated: true }), ...silent },
      never
    )
    expect(capped.truncated).toBe(false)
    await capped.goTo('/big')
    expect(capped.truncated).toBe(true)
  })
})

describe('removing', () => {
  test('a file goes alone, by its whole path, after it was asked for; the folder is read again', async () => {
    const { browser, fake, calls } = setup()
    await browser.setApps(fake.apps)
    await browser.open(AGENTS)
    await browser.enter('transcripts')
    const entry = browser.entries.find((e) => e.name === 'A.md')!
    await browser.ask(entry)
    expect(browser.asked).toEqual({ entry, inside: null, more: false })
    await browser.remove(entry)
    expect(calls.filter(([m]) => m === 'host.files.remove')).toEqual([
      ['host.files.remove', { app: 'agents', path: 'transcripts/A.md' }]
    ])
    expect(browser.asked).toBeNull()
    expect(names(browser)).toEqual(['nested', 'b.md'])
    expect(browser.removeError).toBeNull()
  })

  test('a folder: the question says how much is in it, and then it goes with all of that', async () => {
    const { browser, fake, calls } = setup()
    await browser.setApps(fake.apps)
    await browser.open(AGENTS)
    const folder = browser.entries.find((e) => e.name === 'transcripts')!
    await browser.ask(folder)
    expect(browser.asked).toEqual({ entry: folder, inside: 3, more: false })
    await browser.remove(folder)
    expect(calls.at(-2)).toEqual([
      'host.files.remove',
      { app: 'agents', path: 'transcripts', recursive: true }
    ])
    expect(names(browser)).toEqual(['Plan.md', 'report.csv'])
  })

  test('a folder that cannot be looked into is still asked about, without a count', async () => {
    const { browser, fake } = setup()
    await browser.setApps(fake.apps)
    await browser.goTo('/')
    const locked = browser.entries.find((e) => e.name === 'locked')!
    await browser.ask(locked)
    expect(browser.asked).toEqual({ entry: locked, inside: null, more: false })
  })

  test('an entry that is already gone is not an error; a remove the host refuses is said and the folder stays', async () => {
    const { browser, fake } = setup()
    await browser.setApps(fake.apps)
    await browser.open(AGENTS)
    const report = browser.entries.find((e) => e.name === 'report.csv')!
    delete fake.files.agents['report.csv'] // the app removed it first
    await browser.remove(report)
    expect(browser.removeError).toBeNull()
    expect(names(browser)).toEqual(['transcripts', 'Plan.md'])

    const plan = { name: 'Plan.md', kind: 'file', size: 12, link: false }
    const refusing = new FileBrowser(
      {
        call: async (method) => {
          if (method === 'host.files.remove')
            throw new HostError(-32003, 'Plan.md: You do not have permission to remove this.')
          return { entries: [plan], truncated: false }
        },
        ...silent
      },
      never
    )
    await refusing.open(AGENTS)
    await refusing.remove(refusing.entries[0])
    expect(refusing.removeError).toBe('Plan.md: You do not have permission to remove this.')
    expect(refusing.entries.map((e) => e.name)).toEqual(['Plan.md'])
  })
})

describe('a new folder', () => {
  test('is made in the open folder under the name typed, and shows in the list', async () => {
    const { browser, fake, calls } = setup()
    await browser.setApps(fake.apps)
    await browser.open(COMPUTER, [...HOME, 'Documents'])
    expect(browser.newFolder).toBeNull()
    browser.startFolder()
    expect([browser.canCreateFolder, browser.folderProblem]).toEqual([false, null]) // nothing typed
    browser.nameFolder('  Scans ')
    expect(browser.canCreateFolder).toBe(true)
    await browser.createFolder()
    expect(calls.filter(([m]) => m === 'host.files.createDir')).toEqual([
      ['host.files.createDir', { path: '/home/me/Documents/Scans' }]
    ])
    expect(browser.newFolder).toBeNull()
    expect(names(browser)).toEqual(['Scans', 'letter.txt'])
  })

  test('a name with a problem is said and never sent', async () => {
    const { browser, fake, calls } = setup()
    await browser.setApps(fake.apps)
    await browser.open(AGENTS)
    browser.startFolder()
    for (const [name, problem] of [
      ['a/b', 'cannot contain'],
      ['..', 'name of its own'],
      ['TRANSCRIPTS', 'already here'],
      ['x'.repeat(256), 'too long']
    ]) {
      browser.nameFolder(name)
      expect(browser.folderProblem).toContain(problem)
      expect(browser.canCreateFolder).toBe(false)
      await browser.createFolder()
    }
    expect(calls.some(([m]) => m === 'host.files.createDir')).toBe(false)
    browser.cancelFolder()
    expect(browser.newFolder).toBeNull()
    expect(folderNameProblem('Reports 2026', ['a'])).toBeNull()
  })

  test('what the host refuses lands on the form, and goes when the name changes', async () => {
    const { browser, fake } = setup()
    await browser.setApps(fake.apps)
    await browser.open(AGENTS)
    browser.startFolder()
    browser.nameFolder('exports')
    fake.files.agents.exports = {} // made elsewhere since the folder was read
    await browser.createFolder()
    expect(browser.folderProblem).toContain('already here')
    expect(browser.newFolder).toBe('exports')
    browser.nameFolder('exports2')
    expect(browser.folderProblem).toBeNull()
  })
})

describe('uploads', () => {
  test('send each file into the open folder, with progress, and the folder shows them when done', async () => {
    const { browser, fake, sent } = setup()
    await browser.setApps(fake.apps)
    await browser.open(COMPUTER, [...HOME, 'Documents'])
    await browser.upload([fileOf('a b.txt', 'hello'), fileOf('c.bin', 'xyz')])
    expect(sent).toEqual([
      '/files/host/home/me/Documents/a%20b.txt',
      '/files/host/home/me/Documents/c.bin'
    ])
    expect(browser.uploads.map((u) => [u.name, u.status, u.progress])).toEqual([
      ['a b.txt', 'done', 1],
      ['c.bin', 'done', 1]
    ])
    expect(names(browser)).toEqual(['a b.txt', 'c.bin', 'letter.txt'])
    browser.dismissUploads()
    expect(browser.uploads).toEqual([])
  })

  test('go to an app’s Documents too', async () => {
    const { browser, fake, sent } = setup()
    await browser.setApps(fake.apps)
    await browser.open(AGENTS)
    await browser.enter('transcripts')
    await browser.upload([fileOf('new.md')])
    expect(sent).toEqual(['/files/app/agents/transcripts/new.md'])
    expect(names(browser)).toContain('new.md')
  })

  test('a name that is taken asks: replace it, keep both, or cancel; the other files go on', async () => {
    const { browser, fake, sent } = setup()
    await browser.setApps(fake.apps)
    await browser.open(COMPUTER, HOME)
    await browser.upload([fileOf('todo.md', 'new text'), fileOf('other.txt')])
    expect(browser.uploads.map((u) => u.status)).toEqual(['conflict', 'done'])
    expect(homeOf(fake)['todo.md']).toBe(3) // not replaced
    const asked = browser.conflict!
    expect(asked.name).toBe('todo.md')

    await browser.resolveConflict(asked.id, 'keep')
    expect(sent.at(-1)).toBe('/files/host/home/me/todo%20(2).md')
    expect([homeOf(fake)['todo.md'], homeOf(fake)['todo (2).md']]).toEqual([3, 8])
    expect(browser.conflict).toBeNull()

    await browser.upload([fileOf('todo.md', 'newer')])
    await browser.resolveConflict(browser.conflict!.id, 'replace')
    expect(sent.at(-1)).toBe('/files/host/home/me/todo.md?replace=true')
    expect(homeOf(fake)['todo.md']).toBe(5)

    await browser.upload([fileOf('todo.md', 'again')])
    const cancelled = browser.conflict!.id
    await browser.resolveConflict(cancelled, 'cancel')
    expect(browser.uploads.some((u) => u.id === cancelled)).toBe(false)
    expect(homeOf(fake)['todo.md']).toBe(5)
  })

  test('the first free name skips what is there and keeps the extension', () => {
    expect(freeName('report.csv', ['report.csv'])).toBe('report (2).csv')
    expect(freeName('report.csv', ['report.csv', 'Report (2).csv'])).toBe('report (3).csv')
    expect(freeName('Makefile', [])).toBe('Makefile (2)')
    expect(freeName('.env', [])).toBe('.env (2)')
  })

  test('a file over the host’s limit is not sent, and a failure is said per file', async () => {
    const { browser, fake, sent } = setup({ uploadError: 'No space left on device' })
    await browser.setApps(fake.apps)
    await browser.open(COMPUTER, HOME)
    const huge = { name: 'huge.iso', size: 64 * 1024 * 1024 + 1, blob: new Blob(['x']) }
    await browser.upload([huge, fileOf('small.txt')])
    expect(sent).toEqual(['/files/host/home/me/small.txt'])
    expect(browser.uploads.map((u) => [u.name, u.status])).toEqual([
      ['huge.iso', 'failed'],
      ['small.txt', 'failed']
    ])
    expect(browser.uploads[0].error).toContain('Larger than 67 MB')
    expect(browser.uploads[1].error).toBe('No space left on device')
    expect(names(browser)).toEqual(['Documents', 'todo.md'])
  })

  test('progress is the fraction sent, and an upload finishes where it started when the person moved on', async () => {
    let finish: (() => void) | null = null
    const { fake } = setup()
    const slow = new FileBrowser(
      { call: (m, p) => fake.call(m, p), onNotification: fake.onNotification },
      (url, file, onProgress) => {
        onProgress(0.4)
        return new Promise((resolve) => {
          finish = () => resolve(fake.upload(url, file, onProgress))
        })
      }
    )
    await slow.setApps(fake.apps)
    await slow.open(COMPUTER, [...HOME, 'Documents'])
    const done = slow.upload([fileOf('slow.bin')])
    await new Promise((r) => setTimeout(r, 0))
    expect([slow.uploads[0].status, slow.uploads[0].progress]).toEqual(['uploading', 0.4])
    await slow.back() // the person leaves the folder while it is on its way
    finish!()
    await done
    expect(slow.uploads[0].status).toBe('done')
    expect(homeOf(fake).Documents).toHaveProperty(['slow.bin'])
    expect(slow.path).toEqual(HOME)
  })
})

test('a size reads as bytes, then in units of a thousand', () => {
  expect([0, 1, 999, 1000, 1500, 18_240, 148_000, 2_500_000, 3e9].map(fileSize)).toEqual([
    '0 bytes',
    '1 byte',
    '999 bytes',
    '1 KB',
    '1.5 KB',
    '18 KB',
    '148 KB',
    '2.5 MB',
    '3 GB'
  ])
})
