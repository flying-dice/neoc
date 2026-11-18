<!--
  @organism SearchField — a search input with a two-way text binding (usually client state: nobody else writes
  it, it only changes what a source's params ask for). Typing writes through the binding's policy (debounced by
  default); Enter commits and fires `submit`; Escape or Cancel clears the text and fires `cancel`. The accessible
  name is `label`, else the placeholder. Molecules: SearchField.
-->
<script lang="ts">
  import { SearchField } from '@agentos/ui/molecules'
  import { TEXT_SYNC } from '../core/bindings'
  import { useView } from '../context'
  import { commonOf, str } from '../props'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  const common = $derived(commonOf(ctx, node, path, scope))
  const binding = $derived(view.bindingOf(node.props?.text, scope))
  const policy = $derived(view.syncPolicy(p('sync'), TEXT_SYNC))
  const value = $derived(str(binding ? binding.value : p('text')))
  const placeholder = $derived(str(p('placeholder'), 'Search'))
  const name = $derived(common.label ?? placeholder)
</script>

<div class="search" style={common.style} title={common.help}>
  <SearchField
    testid={common.testid}
    {value}
    {placeholder}
    label={name}
    aria-description={common.hint}
    disabled={common.disabled}
    showCancel
    oninput={(v) => binding?.set(v, policy)}
    onfocuschange={(focused) => {
      binding?.setFocused(focused)
      if (!focused) void binding?.commit()
    }}
    onsubmit={async (v) => {
      await binding?.commit()
      view.fire(node, path, 'submit', scope, { value: v })
    }}
    oncancel={() => {
      void binding?.commit()
      view.fire(node, path, 'cancel', scope)
    }}
  />
</div>

<style>
  .search {
    display: flex;
    min-width: 0;
  }
</style>
