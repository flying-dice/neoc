<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn, userEvent, waitFor, within } from 'storybook/test'
  import Alert from './Alert.svelte'
  import Popover from './Popover.svelte'
  import Sheet from './Sheet.svelte'
  import Button from '../atoms/Button.svelte'
  import Text from '../atoms/Text.svelte'
  import TextInput from '../atoms/TextInput.svelte'

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
  let sheet = $state(true)
  let alert = $state(true)
  let popover = $state(true)
  let anchor: HTMLElement | null = $state(null)
  let detent = $state<'small' | 'medium' | 'large'>('medium')
  let name = $state('')
</script>

{#snippet backdrop()}
  <div style="padding:16px;display:grid;gap:8px">
    <Text style="title2">Notes</Text>
    <Text color="secondary">The window behind the presentation stays visible, dimmed.</Text>
    <div style="display:flex;gap:8px">
      <Button testid="story-open-sheet" variant="tinted" onclick={() => (sheet = true)}>Open sheet</Button>
      <Button testid="story-open-alert" variant="gray" onclick={() => (alert = true)}>Open alert</Button>
    </div>
  </div>
{/snippet}

{#snippet cancelButton()}<Button testid="sheet-cancel" variant="plain" onclick={() => (sheet = false)}>Cancel</Button>{/snippet}
{#snippet doneButton()}<Button testid="sheet-done" variant="plain" onclick={() => { confirm(); sheet = false }}>Done</Button>{/snippet}

<Story name="Form sheet with cancel and confirm" asChild>
  <div class="stage">
    {@render backdrop()}
    <Sheet testid="sheet" bind:open={sheet} title="New folder" leading={cancelButton} trailing={doneButton} width={360}>
      <div style="padding:16px;display:grid;gap:12px">
        <div style="padding:0 12px;min-height:44px;display:flex;align-items:center;border-radius:var(--radius-md);background:var(--fill-tertiary)">
          <TextInput testid="sheet-name" aria-label="Folder name" placeholder="Folder name" bind:value={name} />
        </div>
        <Text color="secondary" style="footnote">Folders keep related notes together.</Text>
      </div>
    </Sheet>
  </div>
</Story>

<Story
  name="Sheet traps focus and escapes"
  asChild
  play={async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const dialog = await canvas.findByTestId('sheet')
    await waitFor(() => expect(dialog).toHaveFocus())
    await userEvent.tab()
    await expect(canvas.getByTestId('sheet-cancel')).toHaveFocus()
    await userEvent.tab()
    await expect(canvas.getByTestId('sheet-done')).toHaveFocus()
    await userEvent.tab()
    await expect(canvas.getByTestId('sheet-name')).toHaveFocus()
    await userEvent.tab()
    await expect(canvas.getByTestId('sheet-cancel')).toHaveFocus() // wrapped: the background is out of reach
    await userEvent.keyboard('{Escape}')
    await waitFor(() => expect(canvas.queryByTestId('sheet')).toBeNull())
  }}
>
  <div class="stage">
    {@render backdrop()}
    <Sheet testid="sheet" bind:open={sheet} title="New folder" leading={cancelButton} trailing={doneButton} width={360}>
      <div style="padding:16px">
        <div style="padding:0 12px;min-height:44px;display:flex;align-items:center;border-radius:var(--radius-md);background:var(--fill-tertiary)">
          <TextInput testid="sheet-name" aria-label="Folder name" placeholder="Folder name" bind:value={name} />
        </div>
      </div>
    </Sheet>
  </div>
</Story>

<Story name="Page sheet with detents" asChild>
  <div class="stage tall">
    {@render backdrop()}
    <Sheet testid="sheet" bind:open={sheet} presentation="page" detents={['small', 'medium', 'large']} bind:detent title="Details" label="Details">
      <div style="padding:16px;display:grid;gap:8px">
        <Text>Drag the grabber, press Enter on it to cycle, or use the arrow keys.</Text>
        <Text color="secondary" style="footnote">Detent: {detent}</Text>
      </div>
    </Sheet>
  </div>
</Story>

<Story
  name="Detents by keyboard"
  asChild
  play={async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const grabber = await canvas.findByTestId('sheet-grabber')
    const dialog = canvas.getByTestId('sheet')
    await waitFor(() => expect(dialog).toHaveFocus()) // the sheet takes focus first; then move to the grabber
    grabber.focus()
    const height = (): number => Math.round(dialog.getBoundingClientRect().height)
    await waitFor(() => expect(height()).toBe(253)) // medium: 55% of the 460 px window
    await userEvent.keyboard('{ArrowUp}')
    await waitFor(() => expect(height()).toBe(416)) // large: the window minus the 44 px top gap
    await userEvent.keyboard('{ArrowDown}{ArrowDown}')
    await waitFor(() => expect(height()).toBe(160)) // small
  }}
>
  <div class="stage tall">
    {@render backdrop()}
    <Sheet testid="sheet" bind:open={sheet} presentation="page" detents={['small', 'medium', 'large']} bind:detent title="Details" label="Details">
      <div style="padding:16px"><Text>Detent: {detent}</Text></div>
    </Sheet>
  </div>
</Story>

<Story name="Full screen sheet" asChild>
  <div class="stage">
    {@render backdrop()}
    <Sheet testid="sheet" bind:open={sheet} presentation="fullScreen" title="Compose" leading={cancelButton} trailing={doneButton}>
      <div style="padding:16px"><Text>Full screen covers the window.</Text></div>
    </Sheet>
  </div>
</Story>

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

<Story
  name="Alert with prompt field"
  asChild
  play={async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const field = await canvas.findByTestId('alert-field-name')
    await waitFor(() => expect(field).toHaveFocus())
    await userEvent.type(field, 'Groceries{Enter}')
    await waitFor(() => expect(confirm).toHaveBeenCalledWith({ name: 'Groceries' }))
    await waitFor(() => expect(canvas.queryByTestId('alert')).toBeNull())
  }}
>
  <div class="stage">
    {@render backdrop()}
    <Alert
      testid="alert"
      bind:open={alert}
      title="Rename note"
      message="Enter a new name."
      fields={[{ key: 'name', placeholder: 'Name' }]}
      actions={[
        { label: 'Cancel', style: 'cancel' },
        { label: 'Rename', preferred: true, onclick: confirm }
      ]}
    />
  </div>
</Story>

<Story name="Action sheet" asChild>
  <div class="stage">
    {@render backdrop()}
    <Alert
      testid="alert"
      variant="actionSheet"
      bind:open={alert}
      title="Delete 3 notes?"
      message="This cannot be undone."
      actions={[
        { label: 'Delete notes', style: 'destructive', onclick: destroy },
        { label: 'Archive instead', onclick: confirm },
        { label: 'Cancel', style: 'cancel', onclick: cancel }
      ]}
    />
  </div>
</Story>

<Story name="Popover with arrow" asChild>
  <div class="stage">
    <div style="padding:16px;display:flex;justify-content:center;padding-top:24px">
      <button
        type="button"
        data-testid="popover-anchor"
        class="anchor"
        bind:this={anchor}
        aria-haspopup="dialog"
        aria-expanded={popover}
        onclick={() => (popover = !popover)}
      >Filters</button>
    </div>
    <Popover testid="popover" bind:open={popover} {anchor} label="Filters" edge="bottom">
      <div style="padding:14px;display:grid;gap:8px;min-width:200px">
        <Text style="headline">Filters</Text>
        <Text color="secondary" style="footnote">Anchored with an arrow; Escape or a press outside closes it.</Text>
        <Button testid="popover-apply" size="small" onclick={() => (popover = false)}>Apply</Button>
      </div>
    </Popover>
  </div>
</Story>

<Story
  name="Popover closes on Escape and outside press"
  asChild
  play={async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    await canvas.findByTestId('popover')
    await userEvent.keyboard('{Escape}')
    await waitFor(() => expect(canvas.queryByTestId('popover')).toBeNull())
    await userEvent.click(canvas.getByTestId('popover-anchor'))
    await canvas.findByTestId('popover')
    await userEvent.click(canvas.getByTestId('popover-outside'))
    await waitFor(() => expect(canvas.queryByTestId('popover')).toBeNull())
  }}
>
  <div class="stage">
    <div style="padding:16px;display:flex;justify-content:center;gap:40px;padding-top:24px">
      <button type="button" data-testid="popover-anchor" class="anchor" bind:this={anchor} aria-haspopup="dialog" aria-expanded={popover} onclick={() => (popover = !popover)}>Filters</button>
      <button type="button" data-testid="popover-outside" class="anchor">Elsewhere</button>
    </div>
    <Popover testid="popover" bind:open={popover} {anchor} label="Filters">
      <div style="padding:14px"><Button testid="popover-apply" size="small" onclick={() => (popover = false)}>Apply</Button></div>
    </Popover>
  </div>
</Story>

<Story name="Popover collapsed to a sheet (compact)" asChild>
  <div class="stage">
    {@render backdrop()}
    <Popover testid="popover" bind:open={popover} {anchor} label="Filters" compact>
      <div style="padding:16px;display:grid;gap:8px">
        <Text style="headline">Filters</Text>
        <Text color="secondary" style="footnote">On a narrow window a popover becomes a bottom sheet.</Text>
        <Button testid="popover-apply" onclick={() => (popover = false)}>Apply</Button>
      </div>
    </Popover>
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
  .stage.tall {
    height: 460px;
  }
  .anchor {
    appearance: none;
    border: 0;
    min-height: 44px;
    padding: 0 16px;
    border-radius: var(--radius-sm);
    background: var(--fill-tertiary);
    color: var(--label);
    font: var(--type-body);
    cursor: pointer;
  }
</style>
