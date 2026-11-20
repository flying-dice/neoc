<!--
  @molecule Stepper — increments and decrements a number: a scalar two-way binding (each step writes at once,
  human wins on conflict), clamped to `min` / `max` and snapped to `step` (default 1). `title` is the visible label
  and the accessible name. The value is a spinbutton (Up/Down, PageUp/PageDown, Home/End); the minus and plus
  buttons are for pointer and touch. `on.change` reports the new value. Atoms: Stepper, Text.
-->
<script lang="ts">
  import { Stepper, Text } from '@agentos/ui/atoms'
  import { useView } from '../context'
  import { commonOf, str } from '../props'
  import type { AdapterProps } from '../registry'
  import { num, optNum } from '../kit/support'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  const common = $derived(commonOf(ctx, node, path, scope))
  const binding = $derived(view.bindingOf(node.props?.value, scope))
  const title = $derived(str(p('title')))
  const name = $derived(common.label ?? (title || 'Value'))
  const value = $derived(num(binding ? binding.value : p('value'), 0))
  const disabled = $derived(common.disabled || binding === null)

  function onchange(next: number): void {
    binding?.set(next)
    view.fire(node, path, 'change', scope, { value: next })
  }
</script>

<div class="stepper" style={common.style} title={common.help}>
  {#if title}<Text style="body">{title}</Text>{/if}
  <Stepper
    testid={common.testid}
    {value}
    min={optNum(p('min'))}
    max={optNum(p('max'))}
    step={optNum(p('step')) ?? 1}
    {disabled}
    label={name}
    aria-description={common.hint}
    {onchange}
  />
</div>
{#if binding?.error}<p class="error" role="alert">{binding.error}</p>{/if}

<style>
  .stepper {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 44px;
  }
  .error {
    margin: 0;
    font: var(--type-footnote);
    color: var(--ios-red);
  }
</style>
