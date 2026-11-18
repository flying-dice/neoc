<!--
  @organism Picker — one value (or several, `multiple`) from options: a literal list of `{value, label, icon,
  disabled}` or the rows of a source. `style` is `segmented` (a segmented control, arrow keys move the choice),
  `menu` (a button showing the choice that opens a menu with checkmarks), or `inline` (a list with a checkmark;
  `navigation` and `wheel` use it too). A scalar binding: a choice writes at once, human wins on conflict. The
  accessible name is the required `label`. Molecules: OptionList, Menu, Popover. Atoms: SegmentedControl, Button,
  Icon, Text.
-->
<script lang="ts">
  import { Button, Icon, SegmentedControl, Text } from '@agentos/ui/atoms'
  import type { MenuItem } from '@agentos/ui'
  import { Menu, OptionList, Popover } from '@agentos/ui/molecules'
  import { iconOf, pickerOptions } from '../core/menuItems'
  import { choose, selectedKeys, selectionLabel } from '../core/options'
  import { useView } from '../context'
  import { commonOf, oneOf } from '../props'
  import type { AdapterProps } from '../registry'
  import { popupMenu } from '../kit/popupMenu.svelte'
  import { portalToWindow } from '../overlay'
  import type { Json } from '../protocol'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  const popup = popupMenu()
  const common = $derived(commonOf(ctx, node, path, scope))
  const binding = $derived(view.bindingOf(node.props?.selection, scope))
  const current = $derived(binding ? binding.value : p('selection'))
  const multiple = $derived(p('multiple') === true)
  const style = $derived(
    oneOf(p('style'), ['menu', 'segmented', 'wheel', 'inline', 'navigation'] as const, 'menu')
  )
  const options = $derived(pickerOptions(view, node, scope))
  const chosen = $derived(selectedKeys(current, multiple))
  const disabled = $derived(common.disabled || binding === null)
  const name = $derived(common.label ?? 'Choice')
  const closedLabel = $derived(selectionLabel(options, chosen))
  const items = $derived<MenuItem[]>(
    options.map((o) => ({
      key: o.key,
      label: o.label,
      icon: iconOf(o.icon),
      disabled: o.disabled,
      checked: chosen.includes(o.key),
      onclick: () => pick(o.key)
    }))
  )

  function pick(key: string): void {
    const option = options.find((o) => o.key === key)
    if (!option || option.disabled || disabled) return
    const next = choose(current, option.value, multiple)
    binding?.set(next as Json)
    view.fire(node, path, 'change', scope, { value: next as Json })
  }
</script>

<div class="picker" style={common.style} title={common.help}>
  {#if style === 'segmented' && !multiple}
    <SegmentedControl
      testid={common.testid}
      label={name}
      aria-description={common.hint}
      block
      {disabled}
      segments={options.map((o) => ({ value: o.key, label: o.label, disabled: o.disabled }))}
      value={chosen[0]}
      onchange={pick}
    />
  {:else if style === 'inline' || style === 'navigation'}
    <OptionList
      testid={common.testid}
      label={name}
      aria-description={common.hint}
      {multiple}
      {disabled}
      options={options.map((o) => ({
        key: o.key,
        label: o.label,
        icon: iconOf(o.icon),
        disabled: o.disabled
      }))}
      selected={chosen}
      onselect={pick}
    />
  {:else}
    <Button
      testid={common.testid}
      variant="gray"
      {disabled}
      aria-label="{name}: {closedLabel}"
      aria-description={common.hint}
      aria-haspopup="menu"
      aria-expanded={popup.open}
      onclick={() => popup.toggle()}
      onkeydown={(e) => popup.onTriggerKey(e)}
      {@attach (el: HTMLElement) => {
        popup.trigger = el
      }}
    >
      <Text style="subheadline" color="inherit">{closedLabel}</Text>
      <Icon name="chevron.down" size={12} weight={2.8} />
    </Button>
    <div class="portal" {@attach portalToWindow(ctx.app)}>
      <Popover
        testid="{common.testid}-popover"
        kind="menu"
        open={popup.open}
        anchor={popup.trigger ?? null}
        arrow={false}
        onclose={() => popup.close()}
      >
        <Menu
          testid="{common.testid}-menu"
          variant="ios"
          label={name}
          {items}
          autofocus
          onselect={() => {
            if (!multiple) popup.close()
          }}
          onclose={() => popup.close()}
        />
      </Popover>
    </div>
  {/if}
</div>

<style>
  .picker {
    display: flex;
    min-width: 0;
  }
  .portal {
    display: contents;
  }
</style>
