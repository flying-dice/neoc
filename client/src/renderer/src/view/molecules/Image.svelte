<!--
  @molecule Image — an image with a required alternative: `alt`, or `decorative` (then hidden from assistive
  technology). `src` is `{asset}`, `{url}` (https) or `{file}`; whatever cannot load shows the `placeholder` tile.
  `fit` is `fit` (contain) or `fill` (cover), `size` makes a square, `corner` a radius token. Atoms: Picture.
-->
<script lang="ts">
  import { Picture } from '@agentos/ui/atoms'
  import { useView } from '../context'
  import { commonOf, oneOf, str } from '../props'
  import type { AdapterProps } from '../registry'
  import { imageSource } from '../kit/image'
  import { optNum, optStr } from '../kit/support'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const p = (name: string): unknown => ctx.view.evaluate(node.props?.[name], scope)

  const RADII = { none: '0', xs: 'var(--radius-xs)', sm: 'var(--radius-sm)', md: 'var(--radius-md)', lg: 'var(--radius-lg)', xl: 'var(--radius-xl)', full: '999px' } as const

  const common = $derived(commonOf(ctx, node, path, scope))
  const src = $derived(imageSource(p('src')))
  const decorative = $derived(p('decorative') === true)
  const alt = $derived(str(p('alt')) || common.label || '')
  const size = $derived(optNum(p('size')))
  const corner = $derived(RADII[oneOf(p('corner'), Object.keys(RADII) as (keyof typeof RADII)[], 'none')])
</script>

<span class="image" style={common.style} title={common.help}>
  {#if decorative}
    <Picture
      testid={common.testid}
      {src}
      decorative
      fit={oneOf(p('fit'), ['fit', 'fill'] as const, 'fit')}
      {size}
      corner={p('corner') === undefined ? undefined : corner}
      placeholder={optStr(p('placeholder'))}
    />
  {:else}
    <Picture
      testid={common.testid}
      {src}
      {alt}
      fit={oneOf(p('fit'), ['fit', 'fill'] as const, 'fit')}
      {size}
      corner={p('corner') === undefined ? undefined : corner}
      placeholder={optStr(p('placeholder'))}
    />
  {/if}
</span>

<style>
  .image {
    display: inline-flex;
    min-width: 0;
    max-width: 100%;
  }
</style>
