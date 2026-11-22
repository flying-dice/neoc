<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn, userEvent, waitFor, within } from 'storybook/test'
  import Alert from './Alert.svelte'
  import Button from '../atoms/Button.svelte'
  import Text from '../atoms/Text.svelte'

  const { Story } = defineMeta({
    title: 'Molecules/Overlays',
    tags: ['autodocs'],
    parameters: { controls: { disable: true } }
  })

  const confirm = fn()
  const cancel = fn()
  const destroy = fn()
</script>

<script lang="ts">
  let alert = $state(true)
</script>

{#snippet backdrop()}
  <div style="padding:16px;display:grid;gap:8px">
    <Text style="title2">Notes</Text>
    <Text color="secondary">The window behind the presentation stays visible, dimmed.</Text>
    <div style="display:flex;gap:8px">
      <Button testid="story-open-alert" variant="gray" onclick={() => (alert = true)}>Open alert</Button>
    </div>
  </div>
{/snippet}

<Story name="Alert" asChild>
  <div class="stage">
    {@render backdrop()}
    <Alert
      testid="alert"
      bind:open={alert}
      title="Delete this note?"
      message="It will be removed from every device."
      actions={[
        { label: 'Cancel', style: 'cancel', onclick: cancel },
        { label: 'Delete', style: 'destructive', onclick: destroy }
      ]}
    />
  </div>
</Story>

<Story
  name="Alert keyboard"
  asChild
  play={async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const dialog = await canvas.findByTestId('alert')
    await waitFor(() => expect(canvas.getByTestId('alert-action-save')).toHaveFocus()) // the preferred action
    await userEvent.tab()
    await expect(canvas.getByTestId('alert-action-cancel')).toHaveFocus()
    await userEvent.tab()
    await expect(canvas.getByTestId('alert-action-discard')).toHaveFocus()
    await userEvent.tab()
    await expect(canvas.getByTestId('alert-action-save')).toHaveFocus()
    await userEvent.keyboard('{Escape}') // runs the cancel action
    await waitFor(() => expect(canvas.queryByTestId('alert')).toBeNull())
    await expect(cancel).toHaveBeenCalled()
    await expect(dialog).not.toBeInTheDocument()
  }}
>
  <div class="stage">
    {@render backdrop()}
    <Alert
      testid="alert"
      bind:open={alert}
      title="Save changes?"
      message="You have unsaved changes."
      actions={[
        { label: 'Cancel', style: 'cancel', onclick: cancel },
        { label: 'Discard', style: 'destructive', onclick: destroy },
        { label: 'Save', preferred: true, onclick: confirm }
      ]}
    />
  </div>
</Story>

<style>
  .stage {
    position: relative;
    width: 420px;
    height: 320px;
    overflow: hidden;
    border-radius: var(--radius-lg);
    background: var(--bg-grouped);
    box-shadow: inset 0 0 0 var(--hairline) var(--separator);
  }
</style>
