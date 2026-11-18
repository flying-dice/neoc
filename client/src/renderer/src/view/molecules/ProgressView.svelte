<!--
  @molecule ProgressView — progress, determinate (`value` in 0..1) or indeterminate (no value). `linear` is a bar,
  `circular` a ring, `spinner` the activity indicator. The name is `label` (progress bars announce it with their
  percentage). Atoms: ProgressBar, ProgressRing, ActivityIndicator.
-->
<script lang="ts">
  import { ActivityIndicator, ProgressBar, ProgressRing } from '@agentos/ui/atoms'
  import { useView } from '../context'
  import { commonOf, oneOf } from '../props'
  import type { AdapterProps } from '../registry'
  import { optNum } from '../kit/support'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const p = (name: string): unknown => ctx.view.evaluate(node.props?.[name], scope)

  const common = $derived(commonOf(ctx, node, path, scope))
  const value = $derived(optNum(p('value')))
  const style = $derived(oneOf(p('style'), ['linear', 'circular', 'spinner'] as const, 'linear'))
  const label = $derived(common.label ?? (value === undefined ? 'Loading' : 'Progress'))
</script>

<span class="progress" class:linear={style === 'linear'} style={common.style} title={common.help} data-testid={common.testid}>
  {#if style === 'circular'}
    <ProgressRing {value} {label} />
  {:else if style === 'spinner'}
    <ActivityIndicator {label} />
  {:else}
    <ProgressBar {value} {label} />
  {/if}
</span>

<style>
  .progress {
    display: inline-flex;
    align-items: center;
  }
  .linear {
    display: flex;
    width: 100%;
  }
</style>
