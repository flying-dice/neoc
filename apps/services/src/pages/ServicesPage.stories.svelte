<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import { alertButton, alertDialog } from '@vos/storybook/kit'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import definition from '../../openrpc.json'
  import ServicesPage from './ServicesPage.svelte'
  import { LAUNCHD, NO_MANAGER, createFailingBackend, createFakeBackend, type FakeBackend } from '../lib/fakeApi'
  import { Services } from '../lib/services.svelte'

  // the window the contract asks for, and the smallest it allows
  const app = definition['x-app']

  interface Args {
    width: number
    height: number
    /** The backend the story's window talks to: a fake in the page, never a host or a real service manager. A function, so each story has its own. */
    backend: () => FakeBackend
  }

  /** The Services app in its window, on a fake backend: every state a person can bring it to. */
  const { Story } = defineMeta({
    title: 'Pages/Services',
    component: ServicesPage,
    tags: ['app'],
    args: { width: app.window.w, height: app.window.h, backend: () => createFakeBackend() },
    argTypes: { backend: { control: false } },
    render: template
  })

  type Canvas = { getByTestId(id: string): HTMLElement; queryByTestId(id: string): HTMLElement | null }
  /** The kit's controls hold the real one in their shadow tree. */
  const inner = (canvas: Canvas, id: string, selector: string): HTMLElement => canvas.getByTestId(id).shadowRoot!.querySelector<HTMLElement>(selector)!
  const appears = (canvas: Canvas, id: string): Promise<void> => waitFor(() => expect(canvas.getByTestId(id)).toBeVisible())
  const gone = (canvas: Canvas, id: string): Promise<void> => waitFor(() => expect(canvas.queryByTestId(id)).toBeNull())
  const says = (canvas: Canvas, id: string, text: string): Promise<void> => waitFor(() => expect(canvas.getByTestId(id)).toHaveTextContent(text))
</script>

{#snippet template(args: Args)}
  <!-- one state per story, as each window has one -->
  {@const backend = args.backend()}
  {@const services = new Services(backend.rpc, backend.events)}
  <AppWindow title={definition.info.title} width={args.width} height={args.height} minWidth={app.window.minW} minHeight={app.window.minH}>
    <ServicesPage {services} every={60000} />
  </AppWindow>
{/snippet}

<!-- A machine with services of its own: the filters, the list and nothing looked at yet. -->
<Story
  name="A machine with services"
  play={async ({ canvas }) => {
    await says(canvas, 'services-summary', '3 running of 5')
    await appears(canvas, 'services-item-nginx')
    await appears(canvas, 'services-nothing')
  }}
/>

<Story name="In the smallest window" args={{ width: app.window.minW, height: app.window.minH }} />

<Story
  name="Looking at a service"
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'services-item-nginx-open')
    await userEvent.click(canvas.getByTestId('services-item-nginx-open'))
    await says(canvas, 'services-detail-name', 'nginx')
    await says(canvas, 'services-detail-state', 'Running')
  }}
/>

<!-- Stopping a service and starting it again, from the list. -->
<Story
  name="Stopping a service"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'services-item-nginx-stop')
    await userEvent.click(canvas.getByTestId('services-item-nginx-stop'))
    await says(canvas, 'services-item-nginx', 'Stopped')
    await userEvent.click(canvas.getByTestId('services-item-nginx-start'))
    await says(canvas, 'services-item-nginx', 'Running')
  }}
/>

<!-- The operating system will not let the host change this one: the window says so, and the service is as it was. -->
<Story
  name="Permission denied"
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'services-item-systemd-logind-open')
    await userEvent.click(canvas.getByTestId('services-item-systemd-logind-open'))
    await userEvent.click(await waitFor(() => canvas.getByTestId('services-detail-stop')))
    await waitFor(() => expect(canvas.getByTestId('services-detail-notice')).toHaveAttribute('data-status', 'denied'))
    await says(canvas, 'services-detail-state', 'Running')
  }}
/>

<Story
  name="A start that fails"
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'services-item-backup-open')
    await userEvent.click(canvas.getByTestId('services-item-backup-open'))
    await userEvent.click(await waitFor(() => canvas.getByTestId('services-detail-start')))
    await waitFor(() => expect(canvas.getByTestId('services-detail-notice')).toHaveAttribute('data-status', 'failed'))
  }}
/>

<!-- The service manager waits for the operating system: the detail says what it waits for. -->
<Story
  name="Waiting for the operating system"
  args={{ backend: () => createFakeBackend({ pace: 400 }) }}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'services-item-cups-open')
    await userEvent.click(canvas.getByTestId('services-item-cups-open'))
    await userEvent.click(await waitFor(() => canvas.getByTestId('services-detail-start')))
    await appears(canvas, 'services-detail-waiting')
  }}
/>

<Story
  name="Registering a service"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await waitFor(() => canvas.getByTestId('services-register')))
    await appears(canvas, 'services-form-label')
    await userEvent.type(inner(canvas, 'services-form-label', 'input'), 'my-app')
    await userEvent.type(inner(canvas, 'services-form-program', 'input'), '/usr/local/bin/my-app')
    await userEvent.click(canvas.getByTestId('services-form-register'))
    await says(canvas, 'services-detail-name', 'my-app')
  }}
/>

<!-- What was typed is refused: the form's footer says why, and what was typed stays. -->
<Story
  name="A registration is refused"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await waitFor(() => canvas.getByTestId('services-register')))
    await appears(canvas, 'services-form-label')
    await userEvent.type(inner(canvas, 'services-form-label', 'input'), 'my-app')
    await userEvent.type(inner(canvas, 'services-form-program', 'input'), 'relative')
    await userEvent.click(canvas.getByTestId('services-form-register'))
    await waitFor(() => expect(canvas.getByTestId('services-form')).toHaveProperty('footerTone', 'error'))
  }}
/>

<!-- Unregistering asks first, names the service and says the program is kept. -->
<Story
  name="Asked before unregistering"
  tags={['keep-focus']}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'services-item-cups-open')
    await userEvent.click(canvas.getByTestId('services-item-cups-open'))
    await userEvent.click(await waitFor(() => canvas.getByTestId('services-detail-remove')))
    const alert = (): Element | null => alertDialog(canvas.getByTestId('services-confirm'))
    await waitFor(() => expect(alert()).toHaveTextContent('Unregister cups?'))
    await expect(alert()).toHaveTextContent('The program it ran is kept.')
  }}
/>

<Story
  name="Unregistering a service"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'services-item-cups-open')
    await userEvent.click(canvas.getByTestId('services-item-cups-open'))
    await userEvent.click(await waitFor(() => canvas.getByTestId('services-detail-remove')))
    await appears(canvas, 'services-confirm')
    await userEvent.click(alertButton(canvas.getByTestId('services-confirm'), 'action'))
    await gone(canvas, 'services-item-cups')
  }}
/>

<!-- A manager with no description: the settings it cannot keep say Not Supported, the ones it did not report say Not Available. -->
<Story
  name="A manager with fewer settings"
  args={{ backend: () => createFakeBackend({ manager: LAUNCHD }) }}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'services-item-nginx-open')
    await userEvent.click(canvas.getByTestId('services-item-nginx-open'))
    await says(canvas, 'services-detail-description', 'Not Supported')
  }}
/>

<Story
  name="No services yet"
  args={{ backend: () => createFakeBackend({ services: [] }) }}
  play={async ({ canvas }) => {
    await appears(canvas, 'services-empty')
  }}
/>

<!-- A machine with no service manager (a Linux box that was not booted with systemd): the window says why, and offers nothing. -->
<Story
  name="No service manager"
  args={{ backend: () => createFakeBackend({ problem: NO_MANAGER }) }}
  play={async ({ canvas }) => {
    await appears(canvas, 'services-unavailable')
  }}
/>

<Story
  name="The host is not answering"
  args={{ backend: () => createFailingBackend('The host is not answering.') }}
  play={async ({ canvas }) => {
    await says(canvas, 'services-failure', 'The host is not answering.')
  }}
/>
