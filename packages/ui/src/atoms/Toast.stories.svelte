<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn, userEvent, waitFor, within } from 'storybook/test'
  import Toast from './Toast.svelte'

  const { Story } = defineMeta({
    title: 'Atoms/Toast',
    component: Toast,
    tags: ['autodocs'],
    args: { testid: 'toast', text: 'Notes needs more room than half the screen, so it opened on its own.' },
    parameters: { controls: { disable: true } },
    render: template
  })

  const dismiss = fn()
  const hold = fn()
  const release = fn()
</script>

{#snippet template(args: { testid: string; text: string })}
  <div style="width:340px"><Toast {...args} ondismiss={dismiss} /></div>
{/snippet}

<Story
  name="Info"
  play={async ({ canvasElement }) => {
    dismiss.mockClear()
    const canvas = within(canvasElement)
    // Touch target: at least 44 x 44 points.
    const box = canvas.getByTestId('toast-dismiss').getBoundingClientRect()
    await expect(box.width).toBeGreaterThanOrEqual(44)
    await expect(box.height).toBeGreaterThanOrEqual(44)
    await userEvent.click(canvas.getByTestId('toast-dismiss'))
    await waitFor(() => expect(dismiss).toHaveBeenCalledTimes(1))
  }}
/>

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
    // keyboard focus on its button holds it too
    await userEvent.tab()
    await expect(canvas.getByTestId('toast-dismiss')).toHaveFocus()
    await expect(hold).toHaveBeenCalledTimes(2)
    await userEvent.tab()
    await waitFor(() => expect(release).toHaveBeenCalledTimes(2))
  }}
>
  <div style="width:340px">
    <Toast testid="toast" text="Note deleted" onhold={hold} onrelease={release} />
  </div>
</Story>
