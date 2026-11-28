<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect } from 'storybook/test'
  import BuildLog from './BuildLog.svelte'
  import type { RecipeStatus } from '../lib/recipes.svelte'

  interface Args {
    status: RecipeStatus
    lines: readonly string[]
    testid: string
  }

  const LINES = ['Building tools:1', 'Step 1/2 : FROM alpine:3.20', ' ---> 1d34ffeaf190', 'Step 2/2 : RUN apk add --no-cache curl', 'Successfully tagged tools:1']

  /** How a recipe's image stands, and the lines of its build as they come. It keeps nothing. */
  const { Story } = defineMeta({
    title: 'Molecules/BuildLog',
    component: BuildLog,
    tags: ['autodocs'],
    args: { status: { tone: 'none', text: 'Not built yet.' }, lines: [], testid: 'containers-recipe-log' },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:480px;color:var(--label);font:var(--type-body)">
    <BuildLog {...args} />
  </div>
{/snippet}

<!-- Never built: a line, and no log. -->
<Story
  name="Not built"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-recipe-log-status')).toHaveTextContent('Not built yet.')
    await expect(canvas.queryByTestId('containers-recipe-log-lines')).toBeNull()
  }}
/>

<!-- Under way: a spinner, and Docker's lines so far. -->
<Story
  name="Building"
  args={{ status: { tone: 'running', text: 'Building…' }, lines: LINES.slice(0, 3) }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-recipe-log-status')).toHaveTextContent('Building…')
    await expect(canvas.getByTestId('containers-recipe-log-lines')).toHaveTextContent('Step 1/2 : FROM alpine:3.20')
  }}
/>

<Story
  name="Built"
  args={{ status: { tone: 'done', text: 'Built: 1d34ffeaf190' }, lines: [...LINES, 'Built tools:1.'] }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-recipe-log-lines')).toHaveTextContent('Built tools:1.')
  }}
/>

<!-- Failed: Docker's reason, in red, and the log up to where it stopped. -->
<Story
  name="Failed"
  args={{
    status: { tone: 'failed', text: "Failed: The image could not be built: The command '/bin/sh -c exit 3' returned a non-zero code: 3" },
    lines: ['Building broken:1', 'Step 1/2 : FROM alpine:3.20', 'Step 2/2 : RUN exit 3', "The image could not be built: The command '/bin/sh -c exit 3' returned a non-zero code: 3"]
  }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-recipe-log-status')).toHaveTextContent('returned a non-zero code: 3')
  }}
/>
