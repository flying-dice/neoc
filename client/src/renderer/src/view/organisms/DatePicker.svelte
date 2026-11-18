<!--
  @organism DatePicker — a date, a time or both, bound to a wire string (`YYYY-MM-DD`, `HH:mm`, ISO-8601 UTC) and
  shown in the viewer's time zone (`env.timeZone`). `style` is `compact` (native inputs) or `graphical` (a month
  calendar; `wheel` falls back to compact). `min`, `max` and `step` (minutes) constrain it. A scalar binding: a pick
  writes at once, human wins on conflict. Molecules: DatePicker.
-->
<script lang="ts">
  import { DatePicker } from '@agentos/ui/molecules'
  import { useView } from '../context'
  import { commonOf, oneOf, str } from '../props'
  import type { AdapterProps } from '../registry'
  import { optNum, optStr } from '../kit/support'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  const common = $derived(commonOf(ctx, node, path, scope))
  const binding = $derived(view.bindingOf(node.props?.value, scope))
  const value = $derived(str(binding ? binding.value : p('value')))
  const kind = $derived(oneOf(p('kind'), ['date', 'time', 'dateTime'] as const, 'date'))
  const style = $derived(oneOf(p('style'), ['compact', 'graphical', 'wheel'] as const, 'compact'))

  function onchange(next: string): void {
    binding?.set(next)
    view.fire(node, path, 'change', scope, { value: next })
  }
</script>

<div class="date" style={common.style} title={common.help}>
  <DatePicker
    testid={common.testid}
    {value}
    {kind}
    style={style === 'graphical' ? 'graphical' : 'compact'}
    min={optStr(p('min'))}
    max={optStr(p('max'))}
    step={optNum(p('step'))}
    label={common.label ?? 'Date'}
    aria-description={common.hint}
    disabled={common.disabled || binding === null}
    timeZone={view.store.env.timeZone}
    locale={view.store.env.locale}
    {onchange}
  />
</div>
{#if binding?.error}<p class="error" role="alert">{binding.error}</p>{/if}

<style>
  .date {
    display: flex;
    min-width: 0;
    max-width: 100%;
  }
  .error {
    margin: 0;
    font: var(--type-footnote);
    color: var(--ios-red);
  }
</style>
