<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, within } from 'storybook/test'
  import ActivityIndicator from './ActivityIndicator.svelte'
  import Text from './Text.svelte'

  const { Story } = defineMeta({
    title: 'Atoms/Display',
    tags: ['autodocs'],
    parameters: { controls: { disable: true } }
  })
</script>

<Story name="Text styles and colours" asChild>
  <div style="display:grid;gap:6px;width:300px">
    {#each ['largeTitle', 'title1', 'title2', 'title3', 'headline', 'body', 'callout', 'subheadline', 'footnote', 'caption1', 'caption2'] as const as style (style)}
      <Text {style}>{style}</Text>
    {/each}
    <div style="display:flex;flex-wrap:wrap;gap:12px;margin-top:8px">
      <Text color="primary">primary</Text>
      <Text color="secondary">secondary</Text>
      <Text color="tint">tint</Text>
      <Text color="destructive">destructive</Text>
      <Text color="success">success</Text>
      <Text color="warning">warning</Text>
    </div>
  </div>
</Story>

<Story
  name="Activity indicator"
  asChild
  play={async ({ canvasElement }) => {
    // What says the app is busy keeps moving, under Reduce Motion too (the e2e run has it on; base.css stops the rest).
    const busy = within(canvasElement).getByRole('status', { name: 'Working' })
    await expect(busy.getAnimations({ subtree: true }).some((a) => a.playState === 'running')).toBe(true)
  }}
>
  <div style="display:flex;gap:16px;align-items:center">
    <ActivityIndicator label="Working" />
    <ActivityIndicator size="large" label="Opening" />
  </div>
</Story>
