<!--
  @molecule Toggle — a scalar two-way binding: a click is explicit intent, so it writes at once (the user's value
  shows immediately as a draft) and the human wins on conflict. If the host refuses the write (a strict source),
  the server value is adopted and the control shows its error. `style` is `switch` (default), `checkbox` or
  `button` (a pressed-state pill). `title` or the `label` slot is the visible label and the accessible name; the
  whole row is the hit target for the label side. Atoms: Toggle, Text, Children.
-->
<script lang="ts">
  import { Text, Toggle } from '@agentos/ui/atoms'
  import Children from '../atoms/Children.svelte'
  import { useView } from '../context'
  import { commonOf, oneOf, str } from '../props'
  import type { AdapterProps } from '../registry'
  import { appearanceStyle } from '../kit/support'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  const common = $derived(commonOf(ctx, node, path, scope))
  const binding = $derived(view.bindingOf(node.props?.on, scope))
  const checked = $derived(Boolean(binding ? binding.value : p('on')))
  const title = $derived(str(p('title')))
  const labelNodes = $derived(node.slots?.label ?? node.children ?? [])
  const name = $derived(common.label ?? (title || undefined))
  const disabled = $derived(common.disabled || binding === null)
  const variant = $derived(oneOf(p('style'), ['switch', 'checkbox', 'button'] as const, 'switch'))
  const appearance = $derived(appearanceStyle(ctx, node, scope))

  function onchange(next: boolean): void {
    binding?.set(next)
    view.fire(node, path, 'change', scope, { value: next })
  }
</script>

<div
  class="toggle"
  class:button={variant === 'button'}
  style="{common.style};{appearance}"
  title={common.help}
>
  {#if variant !== 'button'}
    {#if labelNodes.length}
      <span class="label"
        ><Children
          nodes={labelNodes}
          {path}
          slotName={node.slots?.label ? 'label' : undefined}
          {scope}
        /></span
      >
    {:else if title}
      <span class="label"><Text style="body">{title}</Text></span>
    {/if}
  {/if}
  <Toggle
    testid={common.testid}
    {checked}
    {disabled}
    {variant}
    {title}
    label={name}
    aria-description={common.hint}
    {onchange}
  />
</div>
{#if binding?.error}<p class="error" role="alert">{binding.error}</p>{/if}

<style>
  .toggle {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 44px;
  }
  .toggle.button {
    justify-content: flex-start;
  }
  .label {
    flex: 1;
    min-width: 0;
  }
  .error {
    margin: 0;
    font: var(--type-footnote);
    color: var(--ios-red);
  }
</style>
