<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor, within } from 'storybook/test'
  import Avatar from './Avatar.svelte'
  import Badge from './Badge.svelte'
  import Picture from './Picture.svelte'
  import ProgressBar from './ProgressBar.svelte'
  import ProgressRing from './ProgressRing.svelte'
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

<Story name="Text lines, selectable, tabular" asChild>
  <div style="display:grid;gap:12px;width:240px">
    <Text lines={1}>A single line that is far too long to fit in this narrow box and so truncates</Text>
    <Text lines={2}>Two lines at most: a longer passage of text that wraps once and then runs out of room, so it ends with an ellipsis here.</Text>
    <Text selectable>Selectable text can be copied.</Text>
    <Text tabular>1 111 111 vs 8 888 888</Text>
  </div>
</Story>

<Story
  name="Display readout"
  asChild
  play={async ({ canvasElement }) => {
    const fit = within(canvasElement).getByTestId('readout-fit').querySelector<HTMLElement>('.fit-text')!
    // The readout scales its font to the box once measured, so no glyph is ever clipped.
    await waitFor(() => expect(fit.scrollWidth).toBeLessThanOrEqual(fit.clientWidth))
    await expect(parseFloat(getComputedStyle(fit).fontSize)).toBeLessThan(56)
    await expect(parseFloat(getComputedStyle(fit).fontSize)).toBeGreaterThanOrEqual(56 * 0.3)
  }}
>
  <div style="display:grid;grid-template-columns:minmax(0,1fr);gap:12px;width:260px;background:#000;color:#fff;padding:12px;border-radius:var(--radius-md)">
    <Text style="display" color="inherit">1,234.5</Text>
    <div data-testid="readout-fit"><Text style="display" color="inherit" fit>123,456,789,012.345678</Text></div>
    <div data-testid="readout-short"><Text style="display" color="inherit" fit>42</Text></div>
  </div>
</Story>

<Story
  name="Display readout follows its text"
  asChild
  play={async ({ canvasElement }) => {
    const fit = within(canvasElement).getByTestId('readout-typed').querySelector<HTMLElement>('.fit-text')!
    await expect(parseFloat(getComputedStyle(fit).fontSize)).toBe(56)
    // Typing: the text changes while the readout is already as wide as its box allows, then must shrink to fit.
    for (const text of ['12', '123,456', '123,456,789']) {
      fit.textContent = text
      await waitFor(() => {
        // the glyphs' own width (a Range, so sub-pixel), not scrollWidth, which rounds: an overshoot of half a pixel
        // is already an ellipsis
        const range = document.createRange()
        range.selectNodeContents(fit)
        expect(range.getBoundingClientRect().width).toBeLessThanOrEqual(fit.getBoundingClientRect().width)
      })
    }
    await expect(parseFloat(getComputedStyle(fit).fontSize)).toBeLessThan(56)
  }}
>
  <div style="width:150px;background:#000;color:#fff;padding:12px;border-radius:var(--radius-md)">
    <div data-testid="readout-typed"><Text style="display" color="inherit" fit>0</Text></div>
  </div>
</Story>

<Story
  name="Display readout, longer than it can shrink"
  asChild
  play={async ({ canvasElement }) => {
    const fit = within(canvasElement).getByTestId('readout-long').querySelector<HTMLElement>('.fit-text')!
    // Past the smallest scale it ends in an ellipsis rather than shrinking further or cutting a digit.
    await waitFor(() => expect(parseFloat(getComputedStyle(fit).fontSize)).toBeCloseTo(56 * 0.3, 0))
    await expect(getComputedStyle(fit).textOverflow).toBe('ellipsis')
    await expect(fit.clientWidth).toBeLessThanOrEqual(fit.parentElement!.clientWidth)
  }}
>
  <div style="width:200px;background:#000;color:#fff;padding:12px;border-radius:var(--radius-md)">
    <div data-testid="readout-long"><Text style="display" color="inherit" fit>123,456,789,012,345,678,901,234,567,890.123456789</Text></div>
  </div>
</Story>

<Story name="Avatar" asChild>
  <div style="display:grid;gap:16px">
    <div style="display:flex;gap:12px;align-items:center">
      {#each ['xs', 'sm', 'md', 'lg', 'xl'] as const as size (size)}
        <Avatar name="Ada Lovelace" {size} testid="avatar-{size}" />
      {/each}
    </div>
    <div style="display:flex;gap:12px;align-items:center">
      <Avatar name="Grace Hopper" tint="yellow" testid="avatar-yellow" />
      <Avatar name="Linus T" tint="#1d3557" testid="avatar-navy" />
      <Avatar name="Margaret H" tint="#f1faee" testid="avatar-pale" />
      <Avatar name="Code reviewer" tint="teal" testid="avatar-teal" />
      <Avatar testid="avatar-anon" label="Unknown agent" />
    </div>
    <div style="display:flex;gap:12px;align-items:center">
      {#each ['online', 'busy', 'away', 'offline'] as const as status (status)}
        <Avatar name="Alan Turing" size="lg" {status} testid="avatar-{status}" />
      {/each}
      <Avatar name="Refused image" image="javascript:alert(1)" testid="avatar-refused" />
    </div>
  </div>
</Story>

<Story name="Picture" asChild>
  <div style="display:flex;gap:12px;align-items:flex-start;flex-wrap:wrap">
    <Picture testid="picture-placeholder" alt="Project cover" placeholder="No cover" size={96} />
    <Picture testid="picture-rounded" alt="Rounded placeholder" placeholder="Photo" size={96} corner="var(--radius-md)" />
    <Picture testid="picture-refused" src="javascript:alert(1)" decorative size={64} />
  </div>
</Story>

<Story name="Progress" asChild>
  <div style="display:grid;gap:16px;width:280px">
    <ProgressBar value={0.35} label="Upload" />
    <ProgressBar label="Preparing" />
    <div style="display:flex;gap:16px;align-items:center">
      <ProgressRing value={0.25} label="25 percent" />
      <ProgressRing value={0.75} size={44} label="75 percent" />
      <ProgressRing size={36} label="Loading" />
      <ProgressRing value={1} size={28} tint="var(--ios-green)" label="Done" />
    </div>
  </div>
</Story>

<Story name="Badge" asChild>
  <div style="display:flex;gap:12px;align-items:center">
    <Badge />
    <Badge value={3} />
    <Badge value={120} />
    <Badge value="New" />
  </div>
</Story>
