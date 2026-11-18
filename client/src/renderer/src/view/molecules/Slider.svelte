<!--
  @molecule Slider — a number in a range: a scalar two-way binding that writes as the thumb moves (human wins on
  conflict). `step`, `ticks`, and `minLabel` / `maxLabel` at the ends; `on.editingChange` reports when the person
  starts and stops adjusting. The name is the required `label`; the value is announced as the number. Atoms:
  Slider, Text.
-->
<script lang="ts">
  import { Slider, Text } from '@agentos/ui/atoms'
  import { useView } from '../context'
  import { commonOf, str } from '../props'
  import type { AdapterProps } from '../registry'
  import { num, optNum, optStr } from '../kit/support'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  const common = $derived(commonOf(ctx, node, path, scope))
  const binding = $derived(view.bindingOf(node.props?.value, scope))
  const min = $derived(num(p('min'), 0))
  const max = $derived(Math.max(min, num(p('max'), 1)))
  const step = $derived(optNum(p('step')) ?? (max - min <= 1 ? (max - min) / 100 : 1))
  const value = $derived(
    Math.min(max, Math.max(min, num(binding ? binding.value : p('value'), min)))
  )
  const ticks = $derived(Math.max(0, Math.round(num(p('ticks'), 0))))
  const minLabel = $derived(optStr(p('minLabel')))
  const maxLabel = $derived(optStr(p('maxLabel')))
  const disabled = $derived(common.disabled || binding === null)

  function oninput(next: number): void {
    binding?.set(next)
  }
</script>

<div class="slider" style={common.style} title={common.help}>
  {#if minLabel}<Text style="footnote" color="secondary">{str(minLabel)}</Text>{/if}
  <Slider
    testid={common.testid}
    {value}
    {min}
    {max}
    {step}
    {ticks}
    {disabled}
    label={common.label}
    aria-description={common.hint}
    {oninput}
    oneditingchange={(editing) => view.fire(node, path, 'editingChange', scope, { editing })}
  />
  {#if maxLabel}<Text style="footnote" color="secondary">{str(maxLabel)}</Text>{/if}
</div>
{#if binding?.error}<p class="error" role="alert">{binding.error}</p>{/if}

<style>
  .slider {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 44px;
    min-width: 0;
  }
  .error {
    margin: 0;
    font: var(--type-footnote);
    color: var(--ios-red);
  }
</style>
