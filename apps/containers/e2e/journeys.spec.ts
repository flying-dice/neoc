/**
 * Containers key user journeys, end to end: the real shell, the real host, the app's own client in a frame of its
 * window, and its Luau backend, which reaches Docker through `vnd:bollard`. The Docker it reaches is the journeys' own
 * (`@vos/docker/fake-docker`: the Engine API in this process), which every test first points the app at as a person would, in
 * the app's address field. So the journeys need no Docker, and what they find is the same on every machine.
 *
 * The app's choice of Docker is kept on the shared host, and the fake's containers are shared too: the tests run one
 * after another, each from the machine as it starts, and the last thing done is to choose this machine's own Docker
 * again.
 */
import type { FrameLocator, Locator, Page } from '@playwright/test'
import { expect, expectNoOverflow, frameOf, openDesktop, openFromHome, resizeWindow, test, windowOf } from '../../../e2e/fixtures'
import { callTool } from '../../../e2e/host/pair'
import { startFakeDocker, type FakeDocker } from '@vos/docker/fake-docker'
import { CONTAINERS, NETWORKS } from './machine'

const ID = 'containers'
const frame = (page: Page): FrameLocator => frameOf(page, ID)
const part = (page: Page, name: string): Locator => frame(page).getByTestId(`${ID}-${name}`)
const row = (page: Page, name: string, piece?: string): Locator => part(page, `item-${name}${piece ? `-${piece}` : ''}`)
// the kit's text field holds the real input in its shadow tree, and its picker the real select
const address = (page: Page): Locator => part(page, 'host-input').locator('input')
const field = (page: Page, name: string): Locator => part(page, name).locator('input')
const picker = (page: Page, name: string): Locator => part(page, name).locator('select')
// an image the journeys' registry does not have
const MISSING = 'nosuch/app:1'

test.describe.configure({ mode: 'serial' })

let docker: FakeDocker

test.beforeAll(async () => {
  docker = await startFakeDocker({ containers: CONTAINERS, networks: NETWORKS, missingImages: [MISSING] })
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
    expect(docker.container('pihole')?.running).toBe(false)
    await row(page, 'pihole', 'start').click()
    await expect(row(page, 'pihole', 'stop')).toBeVisible()
    expect(docker.container('pihole')?.running).toBe(true)
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
    expect(docker.container('blog-db-1')?.running).toBe(false)
    expect(docker.container('pihole')?.running, 'a container outside the stack is left alone').toBe(true)
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
    // the names of its environment; the values only once they are asked for
    await expect(details).toContainText('SECRET_TOKEN')
    await expect(details).not.toContainText('do-not-show')
    await part(page, 'details-reveal').click()
    await expect(details).toContainText('SECRET_TOKEN=do-not-show')

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

  test('CT6: remove a running container with force, and take a stack down, each after a second look', async ({ page }) => {
    await openContainers(page)
    await row(page, 'pihole', 'open').click()
    await part(page, 'inspector-remove').click()
    // Docker removes a running container only when forced: the button waits for that
    await expect(part(page, 'remove-confirm')).toHaveAttribute('disabled', '')
    // saying no removes nothing
    await part(page, 'remove-cancel').click()
    await expect(part(page, 'remove')).toHaveCount(0)
    await expect(part(page, 'inspector-name')).toHaveText('pihole')
    expect(docker.container('pihole')?.running).toBe(true)
    await part(page, 'inspector-remove').click()
    await part(page, 'remove-force').click()
    await part(page, 'remove-confirm').click()
    await expect(part(page, 'items')).toBeVisible()
    await expect(row(page, 'pihole')).toHaveCount(0)
    expect(docker.container('pihole')).toBeUndefined()
    expect(docker.volumes(), 'its named volume stays').toContain('pihole-data')

    await part(page, 'stack-blog').click()
    await part(page, 'stack-remove').click()
    await part(page, 'stack-confirm-confirm').click()
    await expect(part(page, 'stack-blog')).toHaveCount(0)
    // what was shown is gone: every container is shown instead
    await expect(part(page, 'title')).toHaveText('All Containers')
    await expect(row(page, 'media-jellyfin-1')).toBeVisible()
    expect(docker.container('blog-db-1')).toBeUndefined()
    expect(docker.networks(), "the stack's network went with it, the other stack's and Docker's own stayed").toEqual([
      'bridge',
      'host',
      'none',
      'media_default'
    ])
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

  test('CT10: make a container with a variable, a port, a folder, a volume and a restart policy, and see it run with them', async ({ page }) => {
    await openContainers(page)
    await part(page, 'new').click()
    await expect(part(page, 'form-title')).toHaveText('New Container')
    await field(page, 'form-image').fill('nginx:1.27')
    await field(page, 'form-name').fill('site')
    await part(page, 'form-add-env').click()
    await field(page, 'form-env-0-name').fill('NGINX_HOST')
    await field(page, 'form-env-0-value').fill('example.org')
    await part(page, 'form-add-port').click()
    await field(page, 'form-port-0-host-port').fill('8088')
    await field(page, 'form-port-0-container-port').fill('80')
    await part(page, 'form-add-bind').click()
    await field(page, 'form-mount-0-source').fill('/srv/www')
    await field(page, 'form-mount-0-target').fill('/usr/share/nginx/html')
    await part(page, 'form-mount-0-read-only').click()
    // a volume that is not there yet: named here, made by Docker
    await part(page, 'form-add-volume').click()
    await picker(page, 'form-mount-1-volume').selectOption('+new')
    await field(page, 'form-mount-1-source').fill('site-data')
    await field(page, 'form-mount-1-target').fill('/data')
    await picker(page, 'form-restart').selectOption('on-failure')
    await field(page, 'form-retries').fill('3')
    await part(page, 'form-save').click()

    // the image was pulled first, and the container is looked at once it runs
    await expect(part(page, 'inspector-name')).toHaveText('site')
    await expect(part(page, 'inspector-status')).toContainText('Up 1 second')
    expect(docker.pulled()).toEqual(['nginx:1.27'])
    expect(docker.container('site')).toMatchObject({
      image: 'nginx:1.27',
      env: ['NGINX_HOST=example.org'],
      ports: { '80/tcp': '8088' },
      mounts: [
        { type: 'bind', source: '/srv/www', target: '/usr/share/nginx/html', readOnly: true },
        { type: 'volume', source: 'site-data', target: '/data', readOnly: false }
      ],
      network: 'bridge',
      restart: 'on-failure',
      retries: 3,
      running: true
    })
    expect(docker.volumes()).toContain('site-data')

    await part(page, 'inspector-tab-details').click()
    const details = part(page, 'details')
    await expect(details).toContainText('8088 → 80/tcp')
    await expect(details).toContainText('on-failure, at most 3 times')
    await expect(details).toContainText('/srv/www -> /usr/share/nginx/html (read-only)')
    await expect(details).toContainText('site-data -> /data')
    await expect(details).toContainText('NGINX_HOST=')
    await expect(details).not.toContainText('example.org')
    await part(page, 'inspector-back').click()
    await expect(row(page, 'site', 'ports')).toHaveText('8088:80')
  })

  test('CT11: edit a container to change its port: it is made again, by its new settings, and runs', async ({ page }) => {
    await openContainers(page)
    await row(page, 'pihole', 'open').click()
    await part(page, 'inspector-edit').click()
    await expect(part(page, 'form-recreate')).toContainText('Anything it keeps outside a volume is lost')
    // filled from the container as Docker has it
    await expect(field(page, 'form-image')).toHaveValue('pihole/pihole:2024.07.0')
    await expect(field(page, 'form-port-0-host-port')).toHaveValue('8053')
    await field(page, 'form-port-0-host-port').fill('8054')
    await part(page, 'form-save').click()

    await expect(part(page, 'form')).toHaveCount(0)
    await expect(part(page, 'inspector-name')).toHaveText('pihole')
    await expect(part(page, 'inspector-status')).toContainText('Up 1 second')
    await part(page, 'inspector-tab-details').click()
    await expect(part(page, 'details')).toContainText('8054 → 80/tcp')
    expect(docker.count(), 'made again, not made twice').toBe(4)
    expect(docker.container('pihole')).toMatchObject({
      ports: { '80/tcp': '8054' },
      running: true,
      restart: 'unless-stopped',
      entrypoint: ['docker-entrypoint.sh'],
      cmd: ['serve']
    })
    expect(docker.container('pihole')?.env).toContain('SECRET_TOKEN=do-not-show')
    await part(page, 'inspector-back').click()
    await expect(row(page, 'pihole', 'ports')).toHaveText('8054:80')
  })

  test('CT12: a container that cannot be made says why, and leaves nothing behind', async ({ page }) => {
    await openContainers(page)
    await part(page, 'new').click()
    // what the backend checks itself, in its words
    await field(page, 'form-image').fill('nginx:1.27')
    await part(page, 'form-add-port').click()
    await field(page, 'form-port-0-container-port').fill('70000')
    await part(page, 'form-save').click()
    await expect(part(page, 'form-failure')).toHaveText("Port: the container's port must be a number from 1 to 65535.")
    await part(page, 'form-port-0-remove').click()
    // an image the registry does not have
    await field(page, 'form-image').fill(MISSING)
    await part(page, 'form-save').click()
    await expect(part(page, 'form-failure')).toContainText(`The image ${MISSING} could not be pulled: pull access denied for nosuch/app`)
    await expect(part(page, 'form')).toBeVisible()
    expect(docker.count()).toBe(4)
    expect(docker.pulled()).toEqual([])
    await part(page, 'form-cancel').click()
    await expect(part(page, 'summary')).toHaveText('3 of 4 containers running')
  })

  test('CT13: remove a stopped container of a stack after a second look, which says Compose makes it again', async ({ page }) => {
    await openContainers(page)
    await row(page, 'media-jellyfin-1', 'open').click()
    await part(page, 'inspector-remove').click()
    await expect(part(page, 'remove-about')).toHaveJSProperty(
      'footer',
      'It is the jellyfin service of the Compose stack media. Compose makes it again on the next docker compose up.'
    )
    await expect(part(page, 'remove-force')).toHaveCount(0)
    await part(page, 'remove-confirm').click()
    await expect(part(page, 'items')).toBeVisible()
    await expect(row(page, 'media-jellyfin-1')).toHaveCount(0)
    expect(docker.container('media-jellyfin-1')).toBeUndefined()
    await expect(part(page, 'stack-media')).toHaveCount(0)
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
