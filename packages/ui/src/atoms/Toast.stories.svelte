<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn, userEvent, waitFor, within } from 'storybook/test'
  import Icon from './Icon.svelte'
  import Toast from './Toast.svelte'

  const { Story } = defineMeta({
    title: 'Atoms/Toast',
    component: Toast,
    tags: ['autodocs'],
    args: { testid: 'toast', kind: 'info', text: 'Note moved to Archive' },
    parameters: { controls: { disable: true } },
    render: template
  })

  const action = fn()
  const dismiss = fn()
  const hold = fn()
  const release = fn()
</script>

{#snippet glyph()}<Icon name="info.circle" size={18} weight={2.2} />{/snippet}
{#snippet check()}<Icon name="checkmark" size={18} weight={2.6} />{/snippet}
{#snippet warn()}<Icon name="exclamationmark.triangle" size={18} weight={2.2} />{/snippet}

{#snippet template(args: { testid: string; kind: 'info' | 'success' | 'error' | 'conflict'; text: string })}
  <div style="width:340px"><Toast {...args} icon={glyph} ondismiss={dismiss} /></div>
{/snippet}

<Story name="Info" />

<Story name="Success" asChild>
  <div style="width:340px"><Toast testid="toast" kind="success" text="Saved" icon={check} ondismiss={dismiss} /></div>
</Story>

<Story name="Error" asChild>
  <div style="width:340px"><Toast testid="toast" kind="error" text="Could not reach the host. Try again." icon={warn} ondismiss={dismiss} /></div>
</Story>

<Story name="Conflict" asChild>
  <div style="width:340px"><Toast testid="toast" kind="conflict" text="Your edit replaced a change made elsewhere" icon={warn} ondismiss={dismiss} /></div>
</Story>

<Story
  name="With action"
  asChild
  play={async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    // Touch targets: at least 44 x 44 points.
    for (const id of ['toast-action', 'toast-dismiss']) {
      const box = canvas.getByTestId(id).getBoundingClientRect()
      await expect(box.width).toBeGreaterThanOrEqual(44)
      await expect(box.height).toBeGreaterThanOrEqual(44)
    }
    await userEvent.click(canvas.getByTestId('toast-action'))
    await waitFor(() => expect(action).toHaveBeenCalledTimes(1))
    await userEvent.click(canvas.getByTestId('toast-dismiss'))
    await waitFor(() => expect(dismiss).toHaveBeenCalled())
  }}
>
  <div style="width:340px">
    <Toast testid="toast" kind="success" text="Note deleted" action="Undo" icon={check} onaction={action} ondismiss={dismiss} />
  </div>
</Story>

<!-- WCAG 2.2.1: the owner pauses the expiry while the pointer or focus is on the toast, and restarts it afterwards. -->
<Story
  name="Held while read"
  asChild
  play={async ({ canvasElement }) => {
    hold.mockClear()
    release.mockClear()
    const canvas = within(canvasElement)
    const toast = canvas.getByTestId('toast')
    await userEvent.hover(toast)
    await expect(hold).toHaveBeenCalledTimes(1)
    await userEvent.unhover(toast)
    await expect(release).toHaveBeenCalledTimes(1)
    // keyboard focus on the action holds it too, and moving between its own buttons does not release it
    await userEvent.tab()
    await expect(canvas.getByTestId('toast-action')).toHaveFocus()
    await expect(hold).toHaveBeenCalledTimes(2)
    await userEvent.tab()
    await expect(canvas.getByTestId('toast-dismiss')).toHaveFocus()
    await expect(release).toHaveBeenCalledTimes(1)
    await userEvent.tab()
    await waitFor(() => expect(release).toHaveBeenCalledTimes(2))
  }}
>
  <div style="width:340px">
    <Toast testid="toast" text="Note deleted" action="Undo" icon={glyph} onhold={hold} onrelease={release} />
  </div>
</Story>
