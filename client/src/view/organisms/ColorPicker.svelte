<!--
  @organism ColorPicker — a colour from the palette (tokens), optionally any `#rrggbb` (`custom`) with an opacity
  slider (`opacity`). The value is a palette token or a hex string, a scalar binding: a pick writes at once. The
  accessible name is the required `label`; each swatch is named by its token. Molecules: ColorPicker.
-->
<script lang="ts">
  import { ColorPicker } from '@vos/ui/molecules'
  import { useView } from '../context'
  import { commonOf, str } from '../props'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  const common = $derived(commonOf(ctx, node, path, scope))
  const binding = $derived(view.bindingOf(node.props?.value, scope))
  const value = $derived(str(binding ? binding.value : p('value')))
  const palette = $derived(
    Array.isArray(p('palette'))
      ? (p('palette') as unknown[]).filter((t): t is string => typeof t === 'string')
      : undefined
  )

  function onchange(next: string): void {
    binding?.set(next)
    view.fire(node, path, 'change', scope, { value: next })
  }
</script>

<div class="color" style={common.style} title={common.help}>
  <ColorPicker
    testid={common.testid}
    {value}
    {palette}
    custom={p('custom') === true}
    opacity={p('opacity') === true}
    label={common.label ?? 'Colour'}
    aria-description={common.hint}
    disabled={common.disabled || binding === null}
    {onchange}
  />
</div>
{#if binding?.error}<p class="error" role="alert">{binding.error}</p>{/if}

<style>
  .color {
    min-width: 0;
    max-width: 100%;
  }
  .error {
    margin: 0;
    font: var(--type-footnote);
    color: var(--ios-red);
  }
</style>
