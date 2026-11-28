/**
 * Pages key user journeys, end to end: the real shell, the real host, the app's own client in a frame of its window,
 * and its Luau backend, which makes the server's container through `vnd:bollard`. The Docker it reaches is the
 * journeys' own (`@vos/docker/fake-docker`: the Engine API in this process), which every test first points the app at as a
 * person would, in the app's address field. So the journeys need no Docker, no web server and no folders, and what
 * they look at is what this app is for: the container it asks Docker for, whose configuration, ports and folders are
 * the sites.
 *
 * The app's configuration is kept on the shared host: the tests run one after another, each starting from no sites,
 * and the last thing done is to choose this machine's own Docker again.
 */
import { sep } from 'node:path'
import type { FrameLocator, Locator, Page } from '@playwright/test'
import { expect, expectNoOverflow, frameOf, openDesktop, openFromHome, resizeWindow, test, windowOf } from '../../../e2e/fixtures'
import { callTool } from '../../../e2e/host/pair'
import { startFakeDocker, type FakeDocker } from '@vos/docker/fake-docker'

const ID = 'pages'
const IMAGE = 'nginx:1.30.0-alpine'
const frame = (page: Page): FrameLocator => frameOf(page, ID)
const part = (page: Page, name: string): Locator => frame(page).getByTestId(`${ID}-${name}`)
// the kit's text field holds the real input in its shadow tree
const field = (page: Page, name: string): Locator => part(page, name).locator('input')
const rows = (page: Page): Locator => frame(page).locator(`ui-row[data-testid^="${ID}-site-"]`)
const names = (page: Page): Locator => rows(page).locator('[data-testid$="-name"]')
/** Something of the row of the site called `name`: its switch, a button, an address. */
const of = (page: Page, name: string, what: string): Locator =>
  rows(page)
    .filter({ has: page.locator('[data-testid$="-name"]', { hasText: name }) })
    .locator(`[data-testid$="-${what}"]`)
const NAME = 'vos-pages'
/** nginx's configuration, as the container's environment carries it. */
const config = (): string => (docker.container(NAME)?.env.find((variable) => variable.startsWith('VOS_NGINX_CONF=')) ?? '').slice('VOS_NGINX_CONF='.length)

test.describe.configure({ mode: 'serial' })

let docker: FakeDocker

test.beforeAll(async () => {
  docker = await startFakeDocker({ startLog: ['nginx/1.30.0', 'start worker processes'] })
})

test.beforeEach(() => docker.reset())

/** Choose a Docker in the address field, wherever it is: in the middle of the window while none is reached, else in the settings. */
async function choose(page: Page, host: string): Promise<void> {
  await expect(part(page, 'state').or(part(page, 'down'))).toBeVisible()
  if (!(await field(page, 'host-input').isVisible())) await part(page, 'tab-settings').click()
  await field(page, 'host-input').fill(host)
  await part(page, 'host-connect').click()
}

/** The change that was asked for is over: the server was made again, or left alone. */
const settled = (page: Page): Promise<void> => expect(part(page, 'working')).toHaveCount(0)

/** Type a new site and add it. */
async function add(page: Page, typed: { name: string; folder: string; port?: string; hostnames?: string }): Promise<void> {
  await part(page, 'add').click()
  await field(page, 'site-name').fill(typed.name)
  await field(page, 'site-folder').fill(typed.folder)
  await field(page, 'site-port').fill(typed.port ?? '')
  await field(page, 'site-hostnames').fill(typed.hostnames ?? '')
  await part(page, 'site-save').click()
}

/** The app's window on the journeys' Docker, as a test starts: no sites, the settings as they come. */
async function openPages(page: Page): Promise<void> {
  await openDesktop(page, { open: [ID] })
  await expect(part(page, 'app')).toBeVisible()
  await choose(page, docker.host)
  await part(page, 'tab-sites').click()
  await expect(part(page, 'sites-summary')).toBeVisible()
  // what an earlier test left behind
  for (let left = await rows(page).count(); left > 0; left -= 1) {
    await rows(page).first().locator('[data-testid$="-remove"]').click()
    await expect(rows(page)).toHaveCount(left - 1)
  }
  await settled(page)
  docker.reset()
  await expect(part(page, 'state')).toHaveText('No sites yet')
}

test.afterAll(async ({ browser }) => {
  const context = await browser.newContext({
    baseURL: test.info().project.use.baseURL,
    storageState: test.info().project.use.storageState as string
  })
  const page = await context.newPage()
  await openPages(page)
  await choose(page, '')
  await expect(part(page, 'down')).toBeVisible()
  await context.close()
  await docker.close()
})

test.describe('Pages journeys', () => {
  test('PG1: open Pages from the Dock, choose a Docker, and host a folder on a port of its own', async ({ page }) => {
    await openDesktop(page)
    await openFromHome(page, ID)
    await expect(windowOf(page, ID)).toBeVisible()
    await openPages(page)
    await expect(part(page, 'empty')).toBeVisible()
    expect(docker.count(), 'with no site there is no container').toBe(0)

    await add(page, { name: 'Blog', folder: '/srv/sites/blog/', port: '8081' })
    await expect(names(page)).toHaveText(['Blog'])
    await expect(part(page, 'state')).toHaveText('Serving 1 site')
    await expect(part(page, 'attention')).toHaveCount(0)
    await expect(of(page, 'Blog', 'folder')).toHaveText('/srv/sites/blog')
    await expect(of(page, 'Blog', 'open-0')).toContainText(':8081')
    // the container is the server: nginx, given its whole configuration, the site's port published and its folder only read
    expect(docker.pulled(), 'the image is pulled once, when Docker does not have it').toEqual([IMAGE])
    expect(docker.count()).toBe(1)
    const made = docker.container(NAME)!
    expect(made).toMatchObject({ name: 'vos-pages', image: IMAGE, ports: { '8081/tcp': '8081' }, restart: 'unless-stopped', running: true })
    expect(made.entrypoint.join(' ')).toContain('/etc/nginx/nginx.conf')
    expect(made.mounts).toHaveLength(1)
    expect(made.mounts[0]).toMatchObject({ source: '/srv/sites/blog', readOnly: true })
    expect(config()).toContain('listen 8081;')
    expect(config()).toContain(`root ${made.mounts[0]!.target};`)
    expect(config(), 'no site has a name: nothing listens on the shared port').not.toContain('default_server')
  })

  test('PG2: a site with names is served on the shared port, and a change or a removal is the container made again', async ({ page }) => {
    await openPages(page)
    await add(page, { name: 'Blog', folder: '/srv/sites/blog', port: '8081', hostnames: 'Blog.Home, www.blog.home' })
    await expect(part(page, 'state')).toHaveText('Serving 1 site')
    await expect.poll(() => docker.container(NAME)?.ports).toEqual({ '80/tcp': '80', '8081/tcp': '8081' })
    expect(config()).toContain('server_name blog.home www.blog.home;')
    // a name that is nobody's is not answered with the first site
    expect(config()).toContain('listen 80 default_server;')
    await expect(of(page, 'Blog', 'open-1')).toHaveText('blog.home')

    await add(page, { name: 'Wiki', folder: '/srv/sites/wiki', hostnames: 'wiki.home' })
    await expect(names(page)).toHaveText(['Blog', 'Wiki'])
    await expect(part(page, 'state')).toHaveText('Serving 2 sites')
    await expect.poll(config).toContain('server_name wiki.home;')
    expect(docker.count(), 'made again, not made twice').toBe(1)
    expect(docker.pulled(), 'the image is there already').toEqual([IMAGE])
    expect(docker.container(NAME)?.mounts.map((mount) => mount.source)).toEqual(['/srv/sites/blog', '/srv/sites/wiki'])

    // the Blog moves to another port and drops a name
    await of(page, 'Blog', 'edit').click()
    await expect(field(page, 'site-port')).toHaveValue('8081')
    await field(page, 'site-port').fill('9090')
    await field(page, 'site-hostnames').fill('blog.home')
    await part(page, 'site-save').click()
    await expect.poll(() => docker.container(NAME)?.ports).toEqual({ '80/tcp': '80', '9090/tcp': '9090' })
    expect(config()).not.toContain('www.blog.home')

    await of(page, 'Wiki', 'remove').click()
    await expect(names(page)).toHaveText(['Blog'])
    await expect.poll(config).not.toContain('wiki.home')
    // made again is made, then started: the start comes a moment after the container is there
    await expect.poll(() => docker.container(NAME)?.running).toBe(true)
  })

  test('PG3: a site that cannot be is refused in words, and nothing changes', async ({ page }) => {
    await openPages(page)
    await add(page, { name: 'Blog', folder: '/srv/sites/blog', port: '8081', hostnames: 'blog.home' })
    await expect(part(page, 'state')).toHaveText('Serving 1 site')
    const before = config()

    await add(page, { name: 'Notes', folder: 'sites/notes', port: '8082' })
    await expect(part(page, 'failure')).toContainText('The folder must be a full path')
    await field(page, 'site-folder').fill('/srv/sites/notes')
    await field(page, 'site-port').fill('8081')
    await part(page, 'site-save').click()
    await expect(part(page, 'failure')).toContainText('Port 8081 is the port of Blog.')
    await field(page, 'site-port').fill('')
    await field(page, 'site-hostnames').fill('blog.home')
    await part(page, 'site-save').click()
    await expect(part(page, 'failure')).toContainText('blog.home is a hostname of Blog.')
    // a name cannot say anything else to nginx
    await field(page, 'site-hostnames').fill('notes.home;')
    await part(page, 'site-save').click()
    await expect(part(page, 'failure')).toContainText('is not a hostname')
    expect(config()).toBe(before)

    // what was typed is still there to be put right
    await expect(field(page, 'site-name')).toHaveValue('Notes')
    await field(page, 'site-hostnames').fill('notes.home')
    await part(page, 'site-save').click()
    await expect(part(page, 'failure')).toHaveCount(0)
    await expect(names(page)).toHaveText(['Blog', 'Notes'])
  })

  test('PG4: a port that is taken says so in Docker\'s words, and "Try Again" starts the server once it is free', async ({ page }) => {
    await openPages(page)
    docker.takePort(true)
    await add(page, { name: 'Blog', folder: '/srv/sites/blog', port: '8081' })
    await expect(part(page, 'state')).toHaveText('Not running')
    await expect(part(page, 'attention')).toContainText('port is already allocated')
    // not a failed call: the site is kept for when the server runs
    await expect(part(page, 'failure')).toHaveCount(0)
    await expect(names(page)).toHaveText(['Blog'])

    docker.takePort(false)
    await part(page, 'apply').click()
    await expect(part(page, 'state')).toHaveText('Serving 1 site')
    await expect(part(page, 'attention')).toHaveCount(0)
    // made again is made, then started: the start comes a moment after the container is there
    await expect.poll(() => docker.container(NAME)?.running).toBe(true)
  })

  test('PG5: the shared port is the one Docker publishes for the sites with names', async ({ page }) => {
    await openPages(page)
    await add(page, { name: 'Blog', folder: '/srv/sites/blog', port: '8081', hostnames: 'blog.home' })
    await expect(part(page, 'state')).toHaveText('Serving 1 site')
    await part(page, 'tab-settings').click()
    await expect(part(page, 'image')).toHaveText(IMAGE)
    await expect(field(page, 'port')).toHaveValue('80')
    await field(page, 'port').fill('8080')
    await part(page, 'settings-save').click()
    await expect.poll(() => docker.container(NAME)?.ports).toEqual({ '8080/tcp': '8080', '8081/tcp': '8081' })
    expect(config()).toContain('listen 8080 default_server;')
    await part(page, 'tab-sites').click()
    await expect(of(page, 'Blog', 'open-1')).toHaveText('blog.home:8080')

    // the own port of a site cannot be the shared one too
    await part(page, 'tab-settings').click()
    await field(page, 'port').fill('8081')
    await part(page, 'settings-save').click()
    await expect(part(page, 'failure')).toContainText('Port 8081 is the own port of Blog.')
    expect(config()).toContain('listen 8080 default_server;')

    // back as it comes, for the tests after this one
    await field(page, 'port').fill('80')
    await part(page, 'settings-save').click()
    await expect(part(page, 'failure')).toHaveCount(0)
    await expect.poll(() => docker.container(NAME)?.ports).toEqual({ '80/tcp': '80', '8081/tcp': '8081' })
  })

  test('PG6: a server that stops behind the app is seen with no input, and its log is read as it is written', async ({ page }) => {
    await openPages(page)
    await add(page, { name: 'Blog', folder: '/srv/sites/blog', port: '8081' })
    await expect(part(page, 'state')).toHaveText('Serving 1 site')
    await part(page, 'tab-log').click()
    await expect(part(page, 'log')).toContainText('start worker processes')
    docker.write(NAME, '192.168.1.7 - - "GET /index.html HTTP/1.1" 200 2143')
    await expect(part(page, 'log')).toContainText('GET /index.html')

    docker.stop(NAME)
    await expect(part(page, 'state')).toHaveText('Not running')
    await expect(part(page, 'attention')).toContainText('Its container stopped')
    await part(page, 'apply').click()
    await expect(part(page, 'state')).toHaveText('Serving 1 site')
  })

  test('PG7: an agent reads the sites, hosts one and stops hosting it, and the window and the container follow', async ({ page }) => {
    await openPages(page)
    await callTool(`${ID}__add_site`, { name: 'Docs', folder: '/srv/sites/docs', port: 8085, hostnames: ['Docs.Home'] })
    await expect(names(page)).toHaveText(['Docs'])
    await expect.poll(config).toContain('server_name docs.home;')

    const listed = JSON.parse(await callTool(`${ID}__list_sites`, {})) as {
      server: { state: string; sharedPort: number; problem: string }
      sites: { name: string; folder: string; port: number; hostnames: string[]; enabled: boolean; problem: string }[]
    }
    expect(listed.server).toMatchObject({ state: 'running', sharedPort: 80, problem: '' })
    expect(listed.sites).toEqual([{ name: 'Docs', folder: '/srv/sites/docs', port: 8085, hostnames: ['docs.home'], enabled: true, problem: '' }])

    // what the app refuses a person it refuses an agent, in the same words
    await expect(callTool(`${ID}__add_site`, { name: 'Other', folder: '/srv/sites/other', port: 8085 })).rejects.toThrow('Port 8085 is the port of Docs.')
    await expect(callTool(`${ID}__add_site`, { name: 'Other', folder: '/srv/sites/other' })).rejects.toThrow('A site needs a port, a hostname, or both')
    await callTool(`${ID}__remove_site`, { name: 'Docs' })
    await expect(part(page, 'sites-summary')).toHaveText('No sites')
    await expect.poll(() => docker.count(), 'the last site went, and the container with it').toBe(0)
    await expect(callTool(`${ID}__remove_site`, { name: 'Docs' })).rejects.toThrow('There is no such site.')
  })

  test('PG8: turning the last site off removes the container and keeps the site', async ({ page }) => {
    await openPages(page)
    await add(page, { name: 'Blog', folder: '/srv/sites/blog', port: '8081', hostnames: 'blog.home' })
    await expect(part(page, 'state')).toHaveText('Serving 1 site')
    await of(page, 'Blog', 'enabled').click()
    await expect(part(page, 'state')).toHaveText('Every site is off')
    expect(docker.count()).toBe(0)
    await expect(names(page)).toHaveText(['Blog'])
    // a site that is off is not opened
    await expect(of(page, 'Blog', 'open-0')).toHaveCount(0)
    await part(page, 'tab-log').click()
    await expect(part(page, 'log-none')).toBeVisible()
    // on again, it is the same server
    await part(page, 'tab-sites').click()
    await of(page, 'Blog', 'enabled').click()
    await expect(part(page, 'state')).toHaveText('Serving 1 site')
    expect(config()).toContain('server_name blog.home;')
    expect(docker.container(NAME)?.ports).toEqual({ '80/tcp': '80', '8081/tcp': '8081' })
  })

  test('PG8b: on this machine\'s own Docker a site whose folder is not there says so, and one whose folder is does not', async ({ page }) => {
    await openPages(page)
    // this machine's own Docker is the one whose folders can be looked at; here nothing listens at it
    await choose(page, '')
    await expect(part(page, 'down')).toBeVisible()
    await callTool(`${ID}__add_site`, { name: 'Lost', folder: '/no/such/folder/anywhere', port: 8086 })
    // The app takes a folder as a path from `/`. On Windows that is this folder without its drive, with `/` between the
    // names: a path of the drive the host runs on, which is this checkout's.
    const here = process.platform === 'win32' ? process.cwd().replace(/^[A-Za-z]:/, '').replaceAll(sep, '/') : process.cwd()
    await callTool(`${ID}__add_site`, { name: 'Found', folder: here, port: 8087 })
    const listed = JSON.parse(await callTool(`${ID}__list_sites`, {})) as { sites: { name: string; problem: string }[] }
    expect(listed.sites.map((site) => [site.name, site.problem])).toEqual([
      ['Found', ''],
      ['Lost', 'The folder is not there.']
    ])
    await callTool(`${ID}__remove_site`, { name: 'Lost' })
    await callTool(`${ID}__remove_site`, { name: 'Found' })
  })

  test('PG9: the Pages window at its smallest still works', async ({ page }) => {
    await openPages(page)
    await add(page, { name: 'Blog', folder: '/srv/sites/a-folder-with-quite-a-long-name/blog', port: '8081', hostnames: 'blog.home, www.blog.home' })
    await expect(part(page, 'state')).toHaveText('Serving 1 site')
    await resizeWindow(page, ID, 100, 100)
    const win = (await windowOf(page, ID).boundingBox())!
    expect(Math.round(win.width), 'the minimum width of the contract').toBe(420)
    expect(Math.round(win.height), 'the minimum height of the contract').toBe(440)
    await expectNoOverflow(page.getByTestId(`window-${ID}-content`))
    await expect(names(page)).toHaveText(['Blog'])
    await part(page, 'add').click()
    await expect(part(page, 'site-save')).toBeVisible()
    await expectNoOverflow(page.getByTestId(`window-${ID}-content`))
    await part(page, 'site-cancel').click()
    await part(page, 'tab-settings').click()
    await expect(part(page, 'settings-save')).toBeVisible()
    await expectNoOverflow(page.getByTestId(`window-${ID}-content`))
  })
})
