/**
 * Containers key user journeys, end to end: the real shell, the real host, the app's own client in a frame of its
 * window, and its Luau backend, which reaches Docker through `vnd:bollard`. The Docker it reaches is the journeys' own
 * (`./fakeDocker.ts`: the Engine API in this process), which every test first points the app at as a person would, in
 * the app's address field. So the journeys need no Docker, and what they find is the same on every machine.
 *
 * The app's choice of Docker is kept on the shared host, and the fake's containers are shared too: the tests run one
 * after another, each from the machine as it starts, and the last thing done is to choose this machine's own Docker
 * again.
 */
import type { FrameLocator, Locator, Page } from '@playwright/test'
import { expect, expectNoOverflow, frameOf, openDesktop, openFromHome, resizeWindow, test, windowOf } from '../../../e2e/fixtures'
import { callTool } from '../../../e2e/host/pair'
import { startFakeDocker, type FakeDocker } from './fakeDocker'

const ID = 'containers'
const frame = (page: Page): FrameLocator => frameOf(page, ID)
const part = (page: Page, name: string): Locator => frame(page).getByTestId(`${ID}-${name}`)
const row = (page: Page, name: string, piece?: string): Locator => part(page, `item-${name}${piece ? `-${piece}` : ''}`)
// the kit's text field holds the real input in its shadow tree
const address = (page: Page): Locator => part(page, 'host-input').locator('input')

test.describe.configure({ mode: 'serial' })

let docker: FakeDocker

test.beforeAll(async () => {
  docker = await startFakeDocker()
})

test.beforeEach(() => docker.reset())

/** Choose a Docker in the address field, wherever it is: in the middle of the window while none is reached, else behind the gear. */
async function choose(page: Page, host: string): Promise<void> {
  await expect(part(page, 'items').or(part(page, 'empty')).or(part(page, 'down'))).toBeVisible()
  if (!(await address(page).isVisible())) await part(page, 'choose').click()
  await address(page).fill(host)
  await part(page, 'host-connect').click()
}

/** The app's window, managing the journeys' Docker. */
async function openContainers(page: Page): Promise<void> {
  await openDesktop(page, { open: [ID] })
  await expect(part(page, 'app')).toBeVisible()
  await choose(page, docker.host)
  await expect(part(page, 'engine')).toContainText('Docker 27.3.1')
  await expect(part(page, 'engine')).toContainText(docker.host)
  await expect(row(page, 'pihole')).toBeVisible()
}

test.afterAll(async ({ browser }) => {
  const context = await browser.newContext({
    baseURL: test.info().project.use.baseURL,
    storageState: test.info().project.use.storageState as string
  })
  const page = await context.newPage()
  await openDesktop(page, { open: [ID] })
  await choose(page, '')
  await expect(part(page, 'engine')).toContainText('This machine')
  await context.close()
  await docker.close()
})

test.describe('Containers journeys', () => {
  test('CT1: open Containers from the Dock, choose a Docker, and see what it has', async ({ page }) => {
    await openDesktop(page)
    await openFromHome(page, ID)
    await expect(windowOf(page, ID)).toBeVisible()
    await choose(page, docker.host)
    await expect(part(page, 'title')).toHaveText('All Containers')
    await expect(part(page, 'summary')).toHaveText('3 of 4 containers running')
    await expect(row(page, 'blog-web-1')).toContainText('ghost:5 · Up 2 hours')
    await expect(row(page, 'blog-web-1', 'ports')).toHaveText('8080:2368')
    await expect(row(page, 'media-jellyfin-1')).toContainText('Exited (0) 3 days ago')
    // the stacks are Compose's, read from its labels
    await expect(part(page, 'stack-blog')).toHaveAttribute('aria-label', 'blog, All 2 running')
    await expect(part(page, 'stack-media')).toHaveAttribute('aria-label', 'media, Stopped')
  })

  test('CT2: stop a container from its row, and start it again', async ({ page }) => {
    await openContainers(page)
    await row(page, 'pihole', 'stop').click()
    await expect(row(page, 'pihole', 'start')).toBeVisible()
    await expect(part(page, 'summary')).toHaveText('2 of 4 containers running')
    expect(docker.running('pihole')).toBe(false)
    await row(page, 'pihole', 'start').click()
    await expect(row(page, 'pihole', 'stop')).toBeVisible()
    expect(docker.running('pihole')).toBe(true)
  })

  test('CT3: start, restart and stop a Compose stack as one', async ({ page }) => {
    await openContainers(page)
    await part(page, 'stack-media').click()
    await expect(part(page, 'title')).toHaveText('media')
    await expect(part(page, 'summary')).toHaveText('0 of 1 container running · /srv/media')
    await expect(row(page, 'pihole')).toHaveCount(0)
    await part(page, 'stack-start').click()
    await expect(part(page, 'summary')).toContainText('1 of 1 container running')
    await expect(part(page, 'stack-media')).toHaveAttribute('aria-label', 'media, Running')

    // a restart stops everything, and starts the service the other waits for first
    await part(page, 'stack-blog').click()
    await part(page, 'stack-restart').click()
    await expect.poll(() => docker.started().slice(-2)).toEqual(['blog-db-1', 'blog-web-1'])
    await expect(part(page, 'summary')).toContainText('2 of 2 containers running')

    await part(page, 'stack-stop').click()
    await expect(part(page, 'summary')).toContainText('0 of 2 containers running')
    await expect(part(page, 'stack-blog')).toHaveAttribute('aria-label', 'blog, Stopped')
    expect(docker.running('blog-db-1')).toBe(false)
    expect(docker.running('pihole'), 'a container outside the stack is left alone').toBe(true)
  })

  test('CT4: read the logs of a container as it writes them, and what it was made with', async ({ page }) => {
    await openContainers(page)
    await row(page, 'blog-web-1', 'open').click()
    await expect(part(page, 'inspector-name')).toHaveText('blog-web-1')
    await expect(part(page, 'logs')).toContainText('Ghost is running')
    // new output arrives without being asked for
    docker.write('blog-web-1', 'GET /welcome 200')
    await expect(part(page, 'logs')).toContainText('GET /welcome 200')

    await part(page, 'inspector-tab-details').click()
    const details = part(page, 'details')
    await expect(details).toContainText('docker-entrypoint.sh serve')
    await expect(details).toContainText('unless-stopped')
    await expect(details).toContainText('blog · web')
    await expect(details).toContainText('blog-web-1-data -> /data')
    // the names of its environment, never the values
    await expect(details).toContainText('SECRET_TOKEN')
    await expect(details).not.toContainText('do-not-show')

    await part(page, 'inspector-back').click()
    await expect(row(page, 'pihole')).toBeVisible()
  })

  test('CT5: a container run in a terminal appears with no input', async ({ page }) => {
    await openContainers(page)
    await expect(row(page, 'adhoc')).toHaveCount(0)
    docker.run('adhoc', 'nginx:1.27')
    await expect(row(page, 'adhoc')).toContainText('nginx:1.27')
    await expect(part(page, 'summary')).toHaveText('4 of 5 containers running')
  })

  test('CT6: remove a container, and take a stack down, each after a second look', async ({ page }) => {
    await openContainers(page)
    await row(page, 'pihole', 'open').click()
    await part(page, 'inspector-remove').click()
    await expect(part(page, 'inspector-confirm')).toContainText('It is running')
    // saying no removes nothing
    await part(page, 'inspector-confirm-cancel').click()
    await expect(part(page, 'inspector-confirm')).toHaveCount(0)
    expect(docker.running('pihole')).toBe(true)
    await part(page, 'inspector-remove').click()
    await part(page, 'inspector-confirm-confirm').click()
    await expect(part(page, 'items')).toBeVisible()
    await expect(row(page, 'pihole')).toHaveCount(0)
    expect(docker.running('pihole')).toBeUndefined()

    await part(page, 'stack-blog').click()
    await part(page, 'stack-remove').click()
    await part(page, 'stack-confirm-confirm').click()
    await expect(part(page, 'stack-blog')).toHaveCount(0)
    // what was shown is gone: every container is shown instead
    await expect(part(page, 'title')).toHaveText('All Containers')
    await expect(row(page, 'media-jellyfin-1')).toBeVisible()
    expect(docker.running('blog-db-1')).toBeUndefined()
    expect(docker.networks(), "the stack's network went with it, the other stack's stayed").toEqual(['media_default'])
  })

  test('CT7: an address that is none is refused, and one nothing listens at says so', async ({ page }) => {
    await openContainers(page)
    await choose(page, 'nas.local')
    await expect(part(page, 'failure')).toContainText('must start with unix:///, tcp:// or http://')
    await expect(part(page, 'engine')).toContainText(docker.host)
    // a port nothing listens on
    await address(page).fill('tcp://127.0.0.1:1')
    await part(page, 'host-connect').click()
    await expect(part(page, 'down')).toBeVisible()
    await expect(part(page, 'down')).toHaveJSProperty('message', 'Docker cannot be reached at tcp://127.0.0.1:1.')
    await expect(part(page, 'failure')).toHaveCount(0)
    // and back
    await choose(page, docker.host)
    await expect(row(page, 'pihole')).toBeVisible()
  })

  test('CT8: an agent sees the containers, stops one and starts a stack, and the window follows', async ({ page }) => {
    await openContainers(page)
    const listed = JSON.parse(await callTool(`${ID}__list_containers`, { stack: 'blog' })) as {
      docker: string
      containers: { name: string; state: string; ports: string[] }[]
    }
    expect(listed.docker).toBe('27.3.1')
    expect(listed.containers.map((c) => `${c.name} ${c.state}`)).toEqual(['blog-db-1 running', 'blog-web-1 running'])
    expect(await callTool(`${ID}__container_logs`, { container: 'blog-web-1', lines: 1 })).toContain('listening on 2368')

    await callTool(`${ID}__stop_container`, { container: 'pihole' })
    await expect(row(page, 'pihole', 'start')).toBeVisible()
    await callTool(`${ID}__start_stack`, { stack: 'media' })
    await expect(part(page, 'stack-media')).toHaveAttribute('aria-label', 'media, Running')
    // Docker's own reason comes back to the agent
    await expect(callTool(`${ID}__start_container`, { container: 'nowhere' })).rejects.toThrow('No such container: nowhere')
  })

  test('CT9: the Containers window at its smallest still works', async ({ page }) => {
    await openContainers(page)
    await resizeWindow(page, ID, 100, 100)
    const win = (await windowOf(page, ID).boundingBox())!
    expect(Math.round(win.width), 'the minimum width of the contract').toBe(440)
    expect(Math.round(win.height), 'the minimum height of the contract').toBe(420)
    await expectNoOverflow(page.getByTestId(`window-${ID}-content`))
    // under 700 wide (the kit's split view) the stacks and the containers are one column each: the stacks first
    await expect(part(page, 'stack-blog')).toBeVisible()
    await expect(row(page, 'pihole')).toBeHidden()
    await part(page, 'stack-blog').click()
    await expect(part(page, 'title')).toHaveText('blog')
    await expect(part(page, 'stack-blog')).toBeHidden()
    await row(page, 'blog-db-1', 'open').click()
    await expect(part(page, 'logs')).toContainText('database system is ready')
    await expectNoOverflow(page.getByTestId(`window-${ID}-content`))
    await part(page, 'inspector-back').click()
    await part(page, 'back').click()
    await expect(part(page, 'stack-blog')).toBeVisible()
  })
})
