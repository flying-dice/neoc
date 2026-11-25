<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import definition from '../../openrpc.json'
  import ExamplePage from './ExamplePage.svelte'
  import { createFailingBackend, createFakeBackend, type FakeBackend } from '../lib/fakeApi'
  import { Example, HELLO } from '../lib/example.svelte'

  // the window the contract asks for, and the smallest it allows
  const app = definition['x-app']

  interface Args {
    width: number
    height: number
    /** The backend the story's window talks to: a fake in the page, never a host. A function, so each story has its own. */
    backend: () => FakeBackend
    /** The sidebar row picked when the window opens. */
    picked?: string
  }

  /** The Example app in its window, on a fake backend: every state a person can bring it to. */
  const { Story } = defineMeta({
    title: 'Pages/Example',
    component: ExamplePage,
    tags: ['app'],
    args: { width: app.window.w, height: app.window.h, backend: () => createFakeBackend() },
    argTypes: { backend: { control: false } },
    render: template
  })

  type Canvas = { getByTestId(id: string): HTMLElement; queryByTestId(id: string): HTMLElement | null }
  /** The kit's controls hold the real one in their shadow tree. */
  const inner = (canvas: Canvas, id: string, selector: string): HTMLElement => canvas.getByTestId(id).shadowRoot!.querySelector<HTMLElement>(selector)!
  /** A window's state with a row already picked, as a person would have it. */
  const opened = (backend: FakeBackend, picked?: string): Example => {
    const example = new Example(backend.rpc, backend.events)
    example.select(picked ?? null)
    return example
  }
  const appears = (canvas: Canvas, id: string): Promise<void> => waitFor(() => expect(canvas.getByTestId(id)).toBeVisible())
</script>

{#snippet template(args: Args)}
  <!-- one state per story, as each window has one -->
  {@const backend = args.backend()}
  {@const example = opened(backend, args.picked)}
  <AppWindow title={definition.info.title} width={args.width} height={args.height} minWidth={app.window.minW} minHeight={app.window.minH}>
    <ExamplePage {example} />
  </AppWindow>
{/snippet}

<!-- Just opened: the sidebar, and nothing picked yet. -->
<Story
  name="Just opened"
  play={async ({ canvas }) => {
    await appears(canvas, 'example-part-contract')
    await appears(canvas, 'example-nothing')
  }}
/>

<!-- A part looked at: its facts as a grouped list. -->
<Story
  name="A part"
  args={{ picked: 'backend' }}
  play={async ({ canvas }) => {
    await waitFor(() => expect(canvas.getByTestId('example-detail-title')).toHaveTextContent('Backend'))
  }}
/>

<!-- The greeting: type a name, Greet, and the backend's answer comes under the form. -->
<Story
  name="Say Hello"
  args={{ picked: HELLO }}
  play={async ({ canvas }) => {
    await appears(canvas, 'example-hello-name')
    const input = inner(canvas, 'example-hello-name', 'input') as HTMLInputElement
    input.value = 'Ada'
    input.dispatchEvent(new Event('input', { bubbles: true, composed: true }))
    await waitFor(() => expect(canvas.getByTestId('example-hello-greet')).not.toHaveAttribute('disabled'))
    canvas.getByTestId('example-hello-greet').click()
    await waitFor(() => expect(canvas.getByTestId('example-hello-greeting')).toHaveAttribute('heading', "Hello, Ada! This came from the Example app's Luau backend."))
  }}
/>

<Story name="In the smallest window" args={{ width: app.window.minW, height: app.window.minH }} />

<Story name="In the smallest window, a part" args={{ width: app.window.minW, height: app.window.minH, picked: 'contract' }} />

<!-- The host cannot be reached: the window says so, and shows nothing it does not have. -->
<Story
  name="The host is away"
  args={{ backend: () => createFailingBackend('The host is not reachable. vos is trying again.') }}
  play={async ({ canvas }) => {
    await appears(canvas, 'example-failure')
  }}
/>
