<!--
  @molecule Toggle — a scalar two-way binding: a click is explicit intent, so it writes at once (the user's value
  shows immediately as a draft) and the human wins on conflict. If the host refuses the write (a strict source),
  the server value is adopted and the control shows its error. `title` is the visible label and the accessible
  name. Atoms: Toggle, Text.
-->
<script lang="ts">
  import { Text, Toggle } from '@agentos/ui/atoms'
  import { useView } from '../context'
  import { commonOf, str } from '../props'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  const common = $derived(commonOf(ctx, node, path, scope))
  const binding = $derived(view.bindingOf(node.props?.on, scope))
  const checked = $derived(Boolean(binding ? binding.value : p('on')))
  const title = $derived(str(p('title')))
  const name = $derived(common.label ?? (title || undefined))
  const disabled = $derived(common.disabled || binding === null)

  function onchange(next: boolean): void {
    binding?.set(next)
    view.fire(node, path, 'change', scope, { value: next })
  }
</script>

<div class="toggle" style={common.style} title={common.help}>
  {#if title}<Text style="body">{title}</Text>{/if}
  <Toggle testid={common.testid} {checked} {disabled} label={name} {onchange} />
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
  .error {
    margin: 0;
    font: var(--type-footnote);
    color: var(--ios-red);
  }
</style>
