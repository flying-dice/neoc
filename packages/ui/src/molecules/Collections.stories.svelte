<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn, waitFor } from 'storybook/test'
  import Button from '../atoms/Button.svelte'
  import Text from '../atoms/Text.svelte'
  import DisclosureGroup from './DisclosureGroup.svelte'
  import EmptyState from './EmptyState.svelte'
  import SwipeAction from './SwipeAction.svelte'

  const { Story } = defineMeta({
    title: 'Molecules/Collections',
    tags: ['autodocs'],
    parameters: { layout: 'padded' }
  })
</script>

{#snippet action()}<Button testid="empty-action" variant="tinted">Compose</Button>{/snippet}
<Story name="EmptyState" asChild parameters={{ controls: { disable: true } }}>
  <div style="width:360px;background:var(--bg-grouped-elevated);border-radius:10px">
    <EmptyState testid="empty" icon="message" title="No messages" message="New messages appear here." {action} />
  </div>
</Story>

<Story name="EmptyState, title only" asChild parameters={{ controls: { disable: true } }}>
  <div style="width:280px;background:var(--bg-grouped-elevated);border-radius:10px">
    <EmptyState testid="empty-title" title="Nothing here yet" />
  </div>
</Story>

<Story
  name="DisclosureGroup"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas, userEvent }) => {
    const head = canvas.getByRole('button', { name: 'Advanced' })
    await expect(head).toHaveAttribute('aria-expanded', 'false')
    await userEvent.click(head)
    await waitFor(() => expect(head).toHaveAttribute('aria-expanded', 'true'))
    await expect(canvas.getByText('Hidden until opened')).toBeVisible()
    head.focus()
    await userEvent.keyboard('{Enter}')
    await waitFor(() => expect(head).toHaveAttribute('aria-expanded', 'false'))
  }}
>
  <div style="width:360px;background:var(--bg-elevated);border-radius:10px">
    <DisclosureGroup testid="disclosure" label="Advanced">
      <Text>Hidden until opened</Text>
    </DisclosureGroup>
  </div>
</Story>

<Story name="DisclosureGroup, open" asChild parameters={{ controls: { disable: true } }}>
  <div style="width:360px;background:var(--bg-elevated);border-radius:10px">
    <DisclosureGroup testid="disclosure-open" label="Details" expanded>
      <Text color="secondary">Always visible here</Text>
    </DisclosureGroup>
  </div>
</Story>

<Story name="SwipeAction, roles" asChild parameters={{ controls: { disable: true } }}>
  <div style="display:flex;height:64px">
    <SwipeAction testid="action-normal" title="Pin" icon="star" onclick={fn()} />
    <SwipeAction testid="action-destructive" title="Delete" icon="trash" role="destructive" />
    <SwipeAction testid="action-neutral" title="Archive" role="neutral" />
    <SwipeAction testid="action-disabled" title="Disabled" disabled />
  </div>
</Story>
