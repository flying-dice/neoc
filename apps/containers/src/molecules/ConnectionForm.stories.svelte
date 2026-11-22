<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import ConnectionForm from './ConnectionForm.svelte'

  interface Args {
    draft: string
    host: string
    ondraft: (address: string) => void
    onconnect: () => void
  }

  const ondraft = fn<(address: string) => void>()
  const onconnect = fn<() => void>()

  type Canvas = { getByTestId(id: string): HTMLElement }
  /** The kit's text field holds the real input in its shadow tree. */
  const input = (canvas: Canvas): HTMLInputElement => canvas.getByTestId('containers-host-input').shadowRoot!.querySelector('input')!

  /** Which Docker the app manages. It keeps nothing: what is typed goes out, and comes back as `draft`. */
  const { Story } = defineMeta({
    title: 'Molecules/ConnectionForm',
    component: ConnectionForm,
    tags: ['autodocs'],
    args: { draft: '', host: '', ondraft, onconnect },
    beforeEach: () => {
      ondraft.mockClear()
      onconnect.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:420px;padding:12px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <ConnectionForm {...args} />
  </div>
{/snippet}

<!-- This machine's own Docker is chosen and nothing else is typed: the button tries it again. -->
<Story
  name="This machine"
  play={async ({ canvas }) => {
    await expect(input(canvas)).toHaveValue('')
    await expect(canvas.getByTestId('containers-host-connect')).toHaveTextContent('Try Again')
  }}
/>

<!-- An address that is not the one chosen: the button connects to it. -->
<Story
  name="Another address typed"
  args={{ draft: 'tcp://nas.local:2375' }}
  play={async ({ canvas }) => {
    await expect(input(canvas)).toHaveValue('tcp://nas.local:2375')
    await expect(canvas.getByTestId('containers-host-connect')).toHaveTextContent('Connect')
  }}
/>

<Story name="Elsewhere" args={{ draft: 'tcp://nas.local:2375', host: 'tcp://nas.local:2375' }} />

<Story
  name="Typing and connecting"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await userEvent.type(input(canvas), 'tcp://a')
    await expect(ondraft).toHaveBeenLastCalledWith('tcp://a')
    await userEvent.click(canvas.getByTestId('containers-host-connect'))
    await expect(onconnect).toHaveBeenCalledTimes(1)
    await userEvent.type(input(canvas), '{Enter}')
    await expect(onconnect).toHaveBeenCalledTimes(2)
  }}
/>
