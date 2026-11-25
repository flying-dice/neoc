/**
 * DNS key user journeys, end to end: the real shell, the real host, the app's own client in a frame of its window,
 * and its Luau backend, which makes the server's container through `vnd:bollard`. The Docker it reaches is the
 * journeys' own (`@vos/docker/fake-docker`: the Engine API in this process), which every test first points the app at as a
 * person would, in the app's address field. So the journeys need no Docker and no DNS server, and what they look at
 * is what this app is for: the container it asks Docker for, whose arguments are the server's whole configuration.
 *
 * The app's configuration is kept on the shared host: the tests run one after another, each starting from a server
 * that is off with no names, and the last thing done is to choose this machine's own Docker again.
 */
import type { FrameLocator, Locator, Page } from '@playwright/test'
import { expect, expectNoOverflow, frameOf, openDesktop, openFromHome, resizeWindow, test, windowOf } from '../../../e2e/fixtures'
import { callTool } from '../../../e2e/host/pair'
import { startFakeDocker, type FakeDocker } from '@vos/docker/fake-docker'

const ID = 'dns'
const IMAGE = '4km3/dnsmasq:2.90-r3-alpine-3.22'
const frame = (page: Page): FrameLocator => frameOf(page, ID)
const part = (page: Page, name: string): Locator => frame(page).getByTestId(`${ID}-${name}`)
// the kit's text field holds the real input in its shadow tree
const field = (page: Page, name: string): Locator => part(page, name).locator('input')
// the name of each row (`dns-record-<id>-name`), not the form's field for a new one (`dns-record-name`)
const records = (page: Page): Locator => frame(page).locator(`[data-testid^="${ID}-record-"][data-testid$="-name"]:not([data-testid="${ID}-record-name"])`)
const NAME = 'vos-dns'
const cmd = (): string[] => docker.container(NAME)?.cmd ?? []

test.describe.configure({ mode: 'serial' })

let docker: FakeDocker

test.beforeAll(async () => {
  docker = await startFakeDocker({ startLog: ['dnsmasq: started, version 2.90 cachesize 1000'], logStream: 'stderr' })
})

test.beforeEach(() => docker.reset())

/** Choose a Docker in the address field, wherever it is: in the middle of the window while none is reached, else in the settings. */
async function choose(page: Page, host: string): Promise<void> {
  await expect(part(page, 'state').or(part(page, 'down'))).toBeVisible()
  if (!(await field(page, 'host-input').isVisible())) await part(page, 'tab-settings').click()
  await field(page, 'host-input').fill(host)
  await part(page, 'host-connect').click()
}

/** Turn the server on or off with its switch, and wait for the container to follow. */
async function turn(page: Page, on: boolean): Promise<void> {
  await part(page, 'enabled').click()
  await expect(part(page, 'enabled')).toHaveJSProperty('checked', on)
  await expect(part(page, 'working')).toHaveCount(0)
}

/** Type a name and its address, and add it. */
async function add(page: Page, name: string, address: string, subdomains = false): Promise<void> {
  await field(page, 'record-name').fill(name)
  await field(page, 'record-address').fill(address)
  if (subdomains) await part(page, 'record-subdomains').click()
  await part(page, 'record-add').click()
}

/** The app's window on the journeys' Docker, as a test starts: the server off, no names, the settings as they come. */
async function openDns(page: Page): Promise<void> {
  await openDesktop(page, { open: [ID] })
  await expect(part(page, 'app')).toBeVisible()
  await choose(page, docker.host)
  await part(page, 'tab-records').click()
  await expect(part(page, 'records-summary')).toBeVisible()
  // what an earlier test left behind
  if (await part(page, 'enabled').evaluate((toggle) => (toggle as unknown as { checked: boolean }).checked)) await turn(page, false)
  for (let left = await records(page).count(); left > 0; left -= 1) {
    await frame(page).locator(`[data-testid^="${ID}-record-"][data-testid$="-remove"]`).first().click()
    await expect(records(page)).toHaveCount(left - 1)
  }
  docker.reset()
  await expect(part(page, 'state')).toHaveText('Off')
}

test.afterAll(async ({ browser }) => {
  const context = await browser.newContext({
    baseURL: test.info().project.use.baseURL,
    storageState: test.info().project.use.storageState as string
  })
  const page = await context.newPage()
  await openDns(page)
  await choose(page, '')
  await expect(part(page, 'down')).toBeVisible()
  await context.close()
  await docker.close()
})

test.describe('DNS journeys', () => {
  test('DN1: open DNS from the Dock, choose a Docker, and turn the server on', async ({ page }) => {
    await openDesktop(page)
    await openFromHome(page, ID)
    await expect(windowOf(page, ID)).toBeVisible()
    await openDns(page)
    await expect(part(page, 'empty')).toBeVisible()
    expect(docker.count(), 'a server that is off has no container').toBe(0)

    await turn(page, true)
    await expect(part(page, 'state')).toHaveText('Answering on port 53')
    await expect(part(page, 'attention')).toHaveCount(0)
    // the container is the server: dnsmasq, told everything on its command line, on the DNS port for both protocols
    expect(docker.pulled(), 'the image is pulled once, when Docker does not have it').toEqual([IMAGE])
    expect(docker.count()).toBe(1)
    expect(docker.container(NAME)).toMatchObject({
      name: 'vos-dns',
      image: IMAGE,
      entrypoint: ['dnsmasq'],
      ports: { '53/udp': '53', '53/tcp': '53' },
      restart: 'unless-stopped',
      running: true
    })
    expect(cmd()).toEqual(expect.arrayContaining(['--keep-in-foreground', '--no-resolv', '--server=1.1.1.1', '--server=1.0.0.1']))
    expect(cmd().some((argument) => argument.startsWith('--host-record='))).toBe(false)
  })

  test('DN2: a name that is added or removed is the container made again with it, or without', async ({ page }) => {
    await openDns(page)
    await turn(page, true)
    await add(page, 'NAS.home', '192.168.1.10')
    await expect(part(page, 'records-summary')).toHaveText('1 name')
    await expect(records(page)).toHaveText(['nas.home'])
    await expect.poll(cmd).toContain('--host-record=nas.home,192.168.1.10')
    // the fields are empty for the next one; a name and every name under it is another kind of argument
    await expect(field(page, 'record-name')).toHaveValue('')
    await add(page, 'apps.home', 'fd00::20', true)
    await expect(records(page)).toHaveText(['apps.home and *.apps.home', 'nas.home'])
    await expect.poll(cmd).toContain('--address=/apps.home/fd00::20')
    expect(docker.count(), 'made again, not made twice').toBe(1)
    expect(docker.pulled(), 'the image is there already').toEqual([IMAGE])
    await expect(part(page, 'state')).toHaveText('Answering on port 53')

    await frame(page).locator(`[data-testid^="${ID}-record-"][data-testid$="-remove"]`).last().click()
    await expect(records(page)).toHaveText(['apps.home and *.apps.home'])
    await expect.poll(cmd).not.toContain('--host-record=nas.home,192.168.1.10')
    // made again is made, then started: the start comes a moment after the container is there
    await expect.poll(() => docker.container(NAME)?.running).toBe(true)
  })

  test('DN3: a name or an address that is none says so in the footer of the form, Add cannot be pressed, and nothing changes', async ({ page }) => {
    await openDns(page)
    await turn(page, true)
    const before = cmd()
    const form = part(page, 'record-form')
    // nothing typed: nothing is wrong, and there is nothing to add
    await expect(form).toHaveJSProperty('footerTone', 'note')
    await expect(part(page, 'record-add')).toHaveJSProperty('disabled', true)

    // a field says what is wrong with it once it is left: in the footer of the form, the first thing first
    await field(page, 'record-name').fill('nas_home')
    await field(page, 'record-address').fill('192.168.1')
    await field(page, 'record-address').blur()
    await expect(form).toHaveJSProperty('footer', 'Only letters, digits, hyphens and dots, like nas.home.')
    await expect(form).toHaveJSProperty('footerTone', 'error')
    await expect(part(page, 'record-name')).toHaveJSProperty('invalid', true)
    await expect(part(page, 'record-address')).toHaveJSProperty('invalid', true)
    // Add cannot be pressed, the window has no banner for it, and nothing was sent
    await expect(part(page, 'record-add')).toHaveJSProperty('disabled', true)
    await expect(part(page, 'failure')).toHaveCount(0)
    expect(cmd()).toEqual(before)

    // the name put right, the footer goes on to the address
    await field(page, 'record-name').fill('nas.home')
    await expect(form).toHaveJSProperty('footer', 'Not an IP address, like 192.168.1.10 or fd00::10.')
    await expect(part(page, 'record-name')).toHaveJSProperty('invalid', false)
    await field(page, 'record-address').fill('--server=6.6.6.6')
    await expect(part(page, 'record-add')).toHaveJSProperty('disabled', true)
    await field(page, 'record-address').fill('192.168.1.10')
    await expect(form).toHaveJSProperty('footerTone', 'note')
    await part(page, 'record-add').click()
    await expect(records(page)).toHaveText(['nas.home'])
    expect(cmd()).toContain('--host-record=nas.home,192.168.1.10')

    // what only the backend knows is said in the same place: the name has that address already
    await add(page, 'nas.home', '192.168.1.10')
    await expect(form).toHaveJSProperty('footer', 'nas.home has the address 192.168.1.10 already.')
    await expect(form).toHaveJSProperty('footerTone', 'error')
    await expect(part(page, 'failure')).toHaveCount(0)
  })

  test('DN4: a port that is taken says so in Docker\'s words, and "Try Again" starts the server once it is free', async ({ page }) => {
    await openDns(page)
    docker.takePort(true)
    await turn(page, true)
    await expect(part(page, 'state')).toHaveText('Not running')
    await expect(part(page, 'attention')).toContainText('port is already allocated')
    // not a failed call: the server is still wanted on, and a name is kept for when it runs
    await expect(part(page, 'failure')).toHaveCount(0)
    await add(page, 'nas.home', '192.168.1.10')
    await expect(records(page)).toHaveText(['nas.home'])
    await expect(part(page, 'state')).toHaveText('Not running')

    docker.takePort(false)
    await part(page, 'apply').click()
    await expect(part(page, 'state')).toHaveText('Answering on port 53')
    await expect(part(page, 'attention')).toHaveCount(0)
    // made again is made, then started: the start comes a moment after the container is there
    await expect.poll(() => docker.container(NAME)?.running).toBe(true)
    expect(cmd()).toContain('--host-record=nas.home,192.168.1.10')
  })

  test('DN5: the settings are the port Docker publishes and what dnsmasq is told', async ({ page }) => {
    await openDns(page)
    await turn(page, true)
    await part(page, 'tab-settings').click()
    await expect(part(page, 'image')).toHaveText(IMAGE)
    await expect(field(page, 'port')).toHaveValue('53')
    await field(page, 'port').fill('5353')
    await field(page, 'upstreams').fill('9.9.9.9, 2620:fe::fe')
    await part(page, 'log-queries').click()
    await part(page, 'settings-save').click()
    await expect(part(page, 'state')).toHaveText('Answering on port 5353')
    await expect.poll(() => docker.container(NAME)?.ports).toEqual({ '53/udp': '5353', '53/tcp': '5353' })
    expect(cmd()).toEqual(expect.arrayContaining(['--server=9.9.9.9', '--server=2620:fe::fe', '--log-queries']))
    expect(cmd()).not.toContain('--server=1.1.1.1')

    // an upstream server that is a name says so in the footer of the form once its field is left, and cannot be saved
    await field(page, 'upstreams').fill('dns.quad9.net')
    await field(page, 'upstreams').blur()
    await expect(part(page, 'settings-form')).toHaveJSProperty('footer', 'dns.quad9.net is not an IP address, like 1.1.1.1 or 2606:4700:4700::1111.')
    await expect(part(page, 'settings-form')).toHaveJSProperty('footerTone', 'error')
    await expect(part(page, 'settings-save')).toHaveJSProperty('disabled', true)
    await expect(part(page, 'failure')).toHaveCount(0)
    expect(cmd()).toContain('--server=9.9.9.9')
    // a port that is none says so as it is typed, before anything else
    await field(page, 'port').fill('70000')
    await expect(part(page, 'settings-form')).toHaveJSProperty('footer', 'A number from 1 to 65535.')
    await expect(part(page, 'settings-save')).toHaveJSProperty('disabled', true)

    // back as they come, for the tests after this one
    await field(page, 'port').fill('53')
    await field(page, 'upstreams').fill('1.1.1.1, 1.0.0.1')
    await part(page, 'log-queries').click()
    await part(page, 'settings-save').click()
    await expect(part(page, 'state')).toHaveText('Answering on port 53')
    await expect(part(page, 'failure')).toHaveCount(0)
  })

  test('DN6: a server that stops behind the app is seen with no input, and its log is read as it is written', async ({ page }) => {
    await openDns(page)
    await turn(page, true)
    await part(page, 'tab-log').click()
    await expect(part(page, 'log')).toContainText('dnsmasq: started')
    docker.write(NAME, 'dnsmasq: query[A] nas.home from 192.168.1.7')
    await expect(part(page, 'log')).toContainText('query[A] nas.home')

    docker.stop(NAME)
    await expect(part(page, 'state')).toHaveText('Not running')
    await expect(part(page, 'attention')).toContainText('Its container stopped')
    await part(page, 'apply').click()
    await expect(part(page, 'state')).toHaveText('Answering on port 53')
  })

  test('DN7: an agent reads the names, adds one and removes it, and the window and the container follow', async ({ page }) => {
    await openDns(page)
    await turn(page, true)
    await callTool(`${ID}__add_record`, { name: 'Printer.Home', address: '192.168.1.31' })
    await expect(records(page)).toHaveText(['printer.home'])
    await expect.poll(cmd).toContain('--host-record=printer.home,192.168.1.31')

    const listed = JSON.parse(await callTool(`${ID}__list_records`, {})) as {
      server: { enabled: boolean; state: string; port: number }
      upstreams: string[]
      records: { name: string; address: string; subdomains: boolean }[]
    }
    expect(listed.server).toMatchObject({ enabled: true, state: 'running', port: 53 })
    expect(listed.upstreams).toEqual(['1.1.1.1', '1.0.0.1'])
    expect(listed.records).toEqual([{ name: 'printer.home', address: '192.168.1.31', subdomains: false }])

    // what the app refuses a person it refuses an agent, in the same words
    await expect(callTool(`${ID}__add_record`, { name: 'printer.home', address: '192.168.1.31' })).rejects.toThrow('has the address 192.168.1.31 already')
    await callTool(`${ID}__remove_record`, { name: 'printer.home' })
    await expect(part(page, 'records-summary')).toHaveText('No names')
    await expect.poll(cmd).not.toContain('--host-record=printer.home,192.168.1.31')
    await expect(callTool(`${ID}__remove_record`, { name: 'printer.home' })).rejects.toThrow('There is no such record.')
  })

  test('DN8: turning the server off removes its container and keeps the names', async ({ page }) => {
    await openDns(page)
    await turn(page, true)
    await add(page, 'nas.home', '192.168.1.10')
    await expect(records(page)).toHaveText(['nas.home'])
    await turn(page, false)
    await expect(part(page, 'state')).toHaveText('Off')
    expect(docker.count()).toBe(0)
    await expect(records(page)).toHaveText(['nas.home'])
    await part(page, 'tab-log').click()
    await expect(part(page, 'log-none')).toBeVisible()
    // on again, it is the same server
    await turn(page, true)
    expect(cmd()).toContain('--host-record=nas.home,192.168.1.10')
  })

  test('DN9: the DNS window at its smallest still works', async ({ page }) => {
    await openDns(page)
    await turn(page, true)
    await add(page, 'nas.home', '192.168.1.10')
    await resizeWindow(page, ID, 100, 100)
    const win = (await windowOf(page, ID).boundingBox())!
    expect(Math.round(win.width), 'the minimum width of the contract').toBe(420)
    expect(Math.round(win.height), 'the minimum height of the contract').toBe(440)
    await expectNoOverflow(page.getByTestId(`window-${ID}-content`))
    await expect(records(page)).toHaveText(['nas.home'])
    await part(page, 'tab-settings').click()
    await expect(part(page, 'settings-save')).toBeVisible()
    await expectNoOverflow(page.getByTestId(`window-${ID}-content`))
  })
})
