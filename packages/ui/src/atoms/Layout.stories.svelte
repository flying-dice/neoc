<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, within } from 'storybook/test'
  import Adaptive from './Adaptive.svelte'
  import Card from './Card.svelte'
  import Divider from './Divider.svelte'
  import Grid from './Grid.svelte'
  import GridCell from './GridCell.svelte'
  import LabeledContent from './LabeledContent.svelte'
  import Scope from './Scope.svelte'
  import ScrollView from './ScrollView.svelte'
  import Spacer from './Spacer.svelte'
  import Stack from './Stack.svelte'
  import Text from './Text.svelte'
  import ZStack from './ZStack.svelte'
  import { gridTemplate, spannedAspect } from '../logic/grid'

  const { Story } = defineMeta({
    title: 'Atoms/Layout',
    tags: ['autodocs'],
    parameters: { layout: 'padded' }
  })
</script>

<Story name="Stack, vertical and horizontal" asChild parameters={{ controls: { disable: true } }}>
  <div style="display:grid;gap:16px;max-width:360px">
    <Stack testid="stack-v" spacing={8} align="start">
      <Text>First</Text>
      <Text>Second</Text>
      <Text>Third</Text>
    </Stack>
    <Stack testid="stack-h" axis="h" spacing={12} align="center" justify="between">
      <Text>Left</Text>
      <Text color="secondary">Middle</Text>
      <Text>Right</Text>
    </Stack>
    <Stack testid="stack-wrap" axis="h" spacing={8} wrap>
      {#each ['alpha', 'beta', 'gamma', 'delta', 'epsilon', 'zeta', 'eta', 'theta'] as w (w)}
        <Card testid="chip-{w}" padding="4px 10px">{w}</Card>
      {/each}
    </Stack>
  </div>
</Story>

<Story name="ZStack, nine anchors" asChild parameters={{ controls: { disable: true } }}>
  <div style="display:grid;grid-template-columns:repeat(3,110px);gap:8px">
    {#each ['topLeading', 'top', 'topTrailing', 'leading', 'center', 'trailing', 'bottomLeading', 'bottom', 'bottomTrailing'] as const as anchor (anchor)}
      <ZStack testid="zstack-{anchor}" align={anchor} style="height:80px;background:var(--fill-tertiary);border-radius:8px">
        <Text style="caption2" color="primary">{anchor}</Text>
        <span style="width:14px;height:14px;border-radius:50%;background:var(--tint)"></span>
      </ZStack>
    {/each}
  </div>
</Story>

<!-- A span-2 cell keeps the row height: its aspect is the grid's, scaled by the span (the Calculator's wide "0" key). -->
<Story
  name="Grid, spans and aspect"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas }) => {
    const single = canvas.getByTestId('cell-A').getBoundingClientRect()
    const wide = canvas.getByTestId('cell-wide').getBoundingClientRect()
    // Two cells wide, one row tall: the same height as a single cell (within the gap).
    await expect(Math.abs(wide.height - single.height)).toBeLessThan(6)
    await expect(wide.width).toBeGreaterThan(single.width * 1.9)
  }}
>
  <div style="max-width:360px">
    <Grid testid="grid" columns={gridTemplate(3)} rowGap={8} colGap={8}>
      {#each ['A', 'B', 'C'] as c (c)}
        <GridCell aspect={spannedAspect(1.4, 1)}><Card testid="cell-{c}" padding="8px">{c}</Card></GridCell>
      {/each}
      <GridCell column="1 / span 2" aspect={spannedAspect(1.4, 2)}><Card testid="cell-wide" padding="8px">Spans two</Card></GridCell>
      <GridCell column="3" aspect={spannedAspect(1.4, 1)}><Card testid="cell-last" padding="8px">Pinned to 3</Card></GridCell>
    </Grid>
  </div>
</Story>

<Story name="Grid, track list" asChild parameters={{ controls: { disable: true } }}>
  <div style="max-width:360px">
    <Grid testid="grid-tracks" columns={gridTemplate([{ fixed: 80 }, { flex: 1 }, { min: 40, max: 90 }])} colGap={8}>
      <Card testid="track-fixed" padding="8px">80 px</Card>
      <Card testid="track-flex" padding="8px">flex</Card>
      <Card testid="track-range" padding="8px">40 to 90</Card>
    </Grid>
  </div>
</Story>

<Story name="Spacer and Divider" asChild parameters={{ controls: { disable: true } }}>
  <div style="max-width:360px;display:grid;gap:12px">
    <Stack testid="bar" axis="h" align="center">
      <Text>Title</Text>
      <Spacer testid="spacer" min={16} />
      <Text color="secondary">Detail</Text>
    </Stack>
    <Divider testid="rule" />
    <Stack testid="bar-v" axis="h" spacing={12} align="center" style="height:32px">
      <Text>One</Text>
      <Divider testid="rule-v" orientation="vertical" />
      <Text>Two</Text>
    </Stack>
  </div>
</Story>

<Story name="Scope, a dark canvas in a light page" asChild parameters={{ controls: { disable: true } }}>
  <div style="display:grid;gap:12px;max-width:320px">
    <Text>Outside the scope</Text>
    <Scope testid="scope-dark" scheme="dark" tint="orange" style="padding:16px;border-radius:14px">
      <Text>Inside: dark scheme, orange tint</Text>
      <Text color="tint">Tinted text</Text>
    </Scope>
  </div>
</Story>

{#snippet compact()}<Text>Compact: one column</Text>{/snippet}
{#snippet regular()}<Text>Regular: side by side</Text>{/snippet}
<Story
  name="Adaptive, by container width"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas }) => {
    await expect(within(canvas.getByTestId('narrow')).getByText(/Compact/)).toBeVisible()
    await expect(within(canvas.getByTestId('wide')).getByText(/Regular/)).toBeVisible()
  }}
>
  <div style="display:grid;gap:12px">
    <div data-testid="narrow" style="width:240px"><Adaptive testid="adaptive-narrow" breakpoint={300} {compact} {regular} /></div>
    <div data-testid="wide" style="width:360px"><Adaptive testid="adaptive-wide" breakpoint={300} {compact} {regular} /></div>
  </div>
</Story>

<Story name="LabeledContent" asChild parameters={{ controls: { disable: true } }}>
  <div style="max-width:360px;background:var(--bg-elevated);border-radius:10px">
    <LabeledContent testid="labeled-version" label="Version" value="1.2.0" />
    <LabeledContent testid="labeled-long" label="Plan" value="Pro, billed yearly, renews on 12 March next year" />
    <LabeledContent testid="labeled-content" label="Status">
      <Text color="secondary">Syncing</Text>
    </LabeledContent>
  </div>
</Story>

<Story name="ScrollView, vertical, horizontal and refresh" asChild parameters={{ controls: { disable: true } }}>
  <div style="display:grid;gap:12px;max-width:360px">
    <ScrollView testid="scroll-v" label="Vertical" style="height:90px;background:var(--bg-elevated);border-radius:10px;padding:8px">
      {#each Array.from({ length: 12 }, (_, i) => i + 1) as n (n)}<Text>Line {n}</Text><br />{/each}
    </ScrollView>
    <ScrollView testid="scroll-h" axis="h" snap label="Horizontal" indicators={false} style="gap:8px">
      {#each Array.from({ length: 8 }, (_, i) => i + 1) as n (n)}
        <Card testid="snap-{n}" size="small" padding="8px">Page {n}</Card>
      {/each}
    </ScrollView>
    <ScrollView testid="scroll-refresh" label="Refreshable" refresh onrefresh={() => new Promise((r) => setTimeout(r, 300))} style="height:90px;background:var(--bg-elevated);border-radius:10px;padding:8px">
      <Text>Pull down, or Tab to the Refresh button</Text>
    </ScrollView>
  </div>
</Story>
