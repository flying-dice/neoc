<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import ConfirmAlert from './ConfirmAlert.svelte'

  interface Args {
    heading: string
    message: string
    action?: string
    testid: string
    onconfirm: () => void
    oncancel: () => void
  }

  const onconfirm = fn<() => void>()
  const oncancel = fn<() => void>()

  /** The second look before something is deleted: an alert over the window. It keeps nothing: the answer goes out. */
  const { Story } = defineMeta({
    title: 'Molecules/ConfirmAlert',
    component: ConfirmAlert,
    tags: ['autodocs'],
    args: {
      heading: 'Delete Website?',
      message: 'Its issues and its pages are deleted with it.',
      testid: 'confirm',
      onconfirm,
      oncancel
    },
    beforeEach: () => {
      onconfirm.mockClear()
      oncancel.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:460px;height:260px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <ConfirmAlert {...args} />
  </div>
{/snippet}

<!-- The alert opens on Cancel: Return and Escape both delete nothing. -->
<Story
  name="Asking"
  tags={['keep-focus']}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('confirm')).toBeVisible()
    await expect(canvas.getByTestId('confirm')).toHaveTextContent('Its issues and its pages are deleted with it.')
    await expect(canvas.getByTestId('confirm-delete')).toHaveTextContent('Delete')
    await expect(canvas.getByTestId('confirm-cancel')).toHaveFocus()
  }}
/>

<Story
  name="Answering"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('confirm-cancel'))
    await expect(oncancel).toHaveBeenCalledTimes(1)
    await expect(onconfirm).not.toHaveBeenCalled()
    await userEvent.click(canvas.getByTestId('confirm-delete'))
    await expect(onconfirm).toHaveBeenCalledTimes(1)
    // Escape is Cancel, from wherever the focus is in the alert
    canvas.getByTestId('confirm-cancel').focus()
    await userEvent.keyboard('{Escape}')
    await expect(oncancel).toHaveBeenCalledTimes(2)
  }}
/>
