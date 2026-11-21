<!--
  @molecule Grid — a grid of equally sized cells: `columns` (a count or a track list), `rows`, the three spacings and
  `cellAspect`. A child's `span`, `rowStart` and `colStart` place it. Atoms: Grid, GridCell, Children.
-->
<script lang="ts">
  import { cellPlacement, gridTemplate } from '@vos/ui'
  import { Grid, GridCell } from '@vos/ui/atoms'
  import Children from '../atoms/Children.svelte'
  import { useView } from '../context'
  import { commonOf, unit } from '../props'
  import { joinStyle, surfaceStyle } from '../style'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const p = (name: string): unknown => ctx.view.evaluate(node.props?.[name], scope)

  const common = $derived(commonOf(ctx, node, path, scope))
  const spacing = $derived(p('spacing') === undefined ? 0 : unit(p('spacing')))
  const cellAspect = $derived(
    Number.isFinite(Number(p('cellAspect'))) ? Number(p('cellAspect')) : undefined
  )
  const placed = $derived(
    (node.children ?? []).map((child) => ({
      child,
      at: cellPlacement({
        span: ctx.view.evaluate(child.props?.span, scope),
        rowStart: ctx.view.evaluate(child.props?.rowStart, scope),
        colStart: ctx.view.evaluate(child.props?.colStart, scope)
      })
    }))
  )
</script>

<Grid
  testid={common.testid}
  columns={gridTemplate(p('columns')) ?? 'repeat(2, minmax(0, 1fr))'}
  rows={gridTemplate(p('rows'))}
  rowGap={p('rowSpacing') === undefined ? spacing : unit(p('rowSpacing'))}
  colGap={p('colSpacing') === undefined ? spacing : unit(p('colSpacing'))}
  style={joinStyle(common.style, surfaceStyle(ctx, node, scope))}
>
  {#each placed as { child, at } (child.key)}
    <GridCell column={at.column} row={at.row} span={at.span} aspect={cellAspect}>
      <Children nodes={[child]} {path} {scope} />
    </GridCell>
  {/each}
</Grid>
