<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import { alertButton, alertDialog } from '@vos/storybook/kit'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import definition from '../../openrpc.json'
  import KeychainPage from './KeychainPage.svelte'
  import { NO_KEYCHAIN, createFailingBackend, createFakeBackend, type FakeBackend } from '../lib/fakeApi'
  import { Keychain } from '../lib/keychain.svelte'

  // the window the contract asks for, and the smallest it allows
  const app = definition['x-app']

  interface Args {
    width: number
    height: number
    /** The backend the story's window talks to: a fake in the page, never a host or a real keychain. A function, so each story has its own. */
    backend: () => FakeBackend
  }

  /** The Keychain app in its window, on a fake backend: every state a person can bring it to. */
  const { Story } = defineMeta({
    title: 'Pages/Keychain',
    component: KeychainPage,
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
  const says = (canvas: Canvas, id: string, text: string): Promise<void> =>
    waitFor(() => expect(canvas.getByTestId(id)).toHaveTextContent(text))

  /** A keychain in which another program keeps a password the app does not show. */
  const withGit = (): FakeBackend => {
    const backend = createFakeBackend()
    backend.outside('github.com', 'ada', 'made by git')
    return backend
  }
</script>

{#snippet template(args: Args)}
  <!-- one state per story, as each window has one -->
  {@const backend = args.backend()}
  {@const keychain = new Keychain(backend.rpc, backend.events)}
  <AppWindow title={definition.info.title} width={args.width} height={args.height} minWidth={app.window.minW} minHeight={app.window.minH}>
    <KeychainPage {keychain} />
  </AppWindow>
{/snippet}

<!-- A home lab: three items, and no password in sight. -->
<Story
  name="A home lab"
  play={async ({ canvas }) => {
    await says(canvas, 'keychain-items-summary', '3 items')
    await appears(canvas, 'keychain-item-3')
    await expect(canvas.queryByTestId('keychain-item-3-password')).toBeNull()
  }}
/>

<Story name="In the smallest window" args={{ width: app.window.minW, height: app.window.minH }} />

<!-- An item is typed and added: it is in the list, and the fields are empty again. -->
<Story
  name="Adding an item"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'keychain-new-service')
    await userEvent.type(inner(canvas, 'keychain-new-service', 'input'), 'nas.home')
    await userEvent.type(inner(canvas, 'keychain-new-account', 'input'), 'ada')
    await userEvent.type(inner(canvas, 'keychain-new-password', 'input'), 'a new secret')
    await userEvent.click(canvas.getByTestId('keychain-new-add'))
    await says(canvas, 'keychain-items-summary', '4 items')
    await says(canvas, 'keychain-item-4-service', 'nas.home')
    await waitFor(() => expect(canvas.getByTestId('keychain-new-service')).toHaveProperty('value', ''))
  }}
/>

<!-- A password is shown when it is asked for, and hidden again. -->
<Story
  name="Showing a password"
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'keychain-item-3-reveal')
    await userEvent.click(canvas.getByTestId('keychain-item-3-reveal'))
    await says(canvas, 'keychain-item-3-password', 'tr0ub4dor&3')
  }}
/>

<Story
  name="Hiding a password"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'keychain-item-3-reveal')
    await userEvent.click(canvas.getByTestId('keychain-item-3-reveal'))
    await appears(canvas, 'keychain-item-3-hide')
    await userEvent.click(canvas.getByTestId('keychain-item-3-hide'))
    await gone(canvas, 'keychain-item-3-password')
  }}
/>

<!-- Deleting asks first: the keychain does not give a password back. -->
<Story
  name="Asked before deleting"
  tags={['keep-focus']}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'keychain-item-3-remove')
    await userEvent.click(canvas.getByTestId('keychain-item-3-remove'))
    // the kit's alert holds its words in its shadow tree
    const alert = (): Element | null => alertDialog(canvas.getByTestId('keychain-confirm'))
    await waitFor(() => expect(alert()).toHaveTextContent('Delete router.home?'))
    await expect(alert()).toHaveTextContent('The password of admin is deleted from the keychain of this machine.')
  }}
/>

<Story
  name="Deleting an item"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'keychain-item-3-remove')
    await userEvent.click(canvas.getByTestId('keychain-item-3-remove'))
    await appears(canvas, 'keychain-confirm')
    await userEvent.click(alertButton(canvas.getByTestId('keychain-confirm'), 'cancel'))
    await gone(canvas, 'keychain-confirm')
    await says(canvas, 'keychain-items-summary', '3 items')
    await userEvent.click(canvas.getByTestId('keychain-item-3-remove'))
    await appears(canvas, 'keychain-confirm')
    await userEvent.click(alertButton(canvas.getByTestId('keychain-confirm'), 'action'))
    await gone(canvas, 'keychain-item-3')
    await says(canvas, 'keychain-items-summary', '2 items')
  }}
/>

<!-- A password another program keeps is not replaced: the form's footer says so, and the window has no banner for it. -->
<Story
  name="The keychain has it already"
  args={{ backend: withGit }}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'keychain-new-service')
    await userEvent.type(inner(canvas, 'keychain-new-service', 'input'), 'github.com')
    await userEvent.type(inner(canvas, 'keychain-new-account', 'input'), 'ada')
    await userEvent.type(inner(canvas, 'keychain-new-password', 'input'), 'a slip')
    await userEvent.click(canvas.getByTestId('keychain-new-add'))
    await waitFor(() => expect(canvas.getByTestId('keychain-new-form')).toHaveProperty('footerTone', 'error'))
    await expect(canvas.getByTestId('keychain-new-form')).toHaveProperty(
      'footer',
      'The keychain has a password for this service and account already. Leave the password empty to show it here.'
    )
    await expect(canvas.queryByTestId('keychain-failure')).toBeNull()
    await says(canvas, 'keychain-items-summary', '3 items')
  }}
/>

<Story
  name="No items yet"
  args={{ backend: () => createFakeBackend({ items: [] }) }}
  play={async ({ canvas }) => {
    await appears(canvas, 'keychain-empty')
  }}
/>

<!-- A machine with no keychain (a Linux box with no Secret Service): the window says why, and offers nothing to type into. -->
<Story
  name="No keychain"
  args={{ backend: () => createFakeBackend({ problem: NO_KEYCHAIN }) }}
  play={async ({ canvas }) => {
    await appears(canvas, 'keychain-unavailable')
    await expect(canvas.queryByTestId('keychain-new-form')).toBeNull()
  }}
/>

<Story
  name="The host is not answering"
  args={{ backend: () => createFailingBackend('The host is not answering.') }}
  play={async ({ canvas }) => {
    await says(canvas, 'keychain-failure', 'The host is not answering.')
  }}
/>
