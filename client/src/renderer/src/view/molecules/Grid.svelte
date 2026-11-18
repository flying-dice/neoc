<!--
  @molecule Grid — a grid of equally sized cells: `columns` (a count or a track list), `rows`, the three spacings and
  `cellAspect`. A child's `span`, `rowStart` and `colStart` place it. Atoms: Grid, GridCell, Children.
-->
<script lang="ts">
  import { cellPlacement, gridTemplate } from '@agentos/ui'
  import { Grid, GridCell } from '@agentos/ui/atoms'
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
  const aspect = $derived(Number(p('cellAspect')))
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
  cellAspect={Number.isFinite(aspect) && aspect > 0 ? aspect : undefined}
  style={joinStyle(common.style, surfaceStyle(ctx, node, scope))}
>
  {#each placed as { child, at } (child.key)}
    <GridCell column={at.column} row={at.row}>
      <Children nodes={[child]} {path} {scope} />
    </GridCell>
  {/each}
</Grid>
