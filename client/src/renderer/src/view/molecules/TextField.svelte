<!--
  @molecule TextField — the two-way text binding in the UI. The input shows the effective value (the user's draft
  first, then the server value) and writes every keystroke into the view's draft overlay; the core debounces the
  write-back, shields the field from remote changes while dirty, and adopts the host's merge on the ack, mapping
  the caret through the diff. Composition (IME) never flushes mid-composition; blur and Enter commit. Keyboard and
  accessibility props (`label`, `hint`, `disabled`, `kind`, `autocapitalize`, `autocorrect`, `submitLabel`) pass
  through to the input. Atoms: TextInput, Children.
-->
<script lang="ts">
  import { TextInput } from '@agentos/ui/atoms'
  import { tick } from 'svelte'
  import Children from '../atoms/Children.svelte'
  import { mapCaret } from '../core/textMerge'
  import { parseRules, validateText } from '../core/validate'
  import { useView } from '../context'
  import { commonOf, oneOf, str } from '../props'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  const KINDS = ['text', 'email', 'url', 'number', 'tel', 'search'] as const
  const SUBMIT = ['enter', 'done', 'go', 'next', 'search', 'send'] as const

  const common = $derived(commonOf(ctx, node, path, scope))
  const binding = $derived(view.bindingOf(node.props?.text, scope))
  const value = $derived(str(binding ? binding.value : p('text')))
  const placeholder = $derived(str(p('placeholder')))
  const kind = $derived(oneOf(p('kind'), KINDS, 'text'))
  const name = $derived(common.label ?? (placeholder || undefined))
  const policy = $derived(view.syncPolicy(p('sync')))
  const rules = $derived(parseRules(p('validate')))
  const readOnly = $derived(binding === null)
  const submitLabel = $derived(oneOf(p('submitLabel'), SUBMIT, 'enter'))

  let input: HTMLInputElement | undefined = $state()
  let touched = $state(false)
  const problem = $derived(touched ? validateText(value, rules, kind === 'number') : null)
  const error = $derived(binding?.error ?? problem ?? undefined)
  const errorId = $derived(`${common.testid}-error`)

  // An ack replaced what was typed with the host's merged value: keep the caret where the person was typing.
  $effect(() => {
    return binding?.onAdopt(({ before, after }) => {
      const el = input
      if (
        !el ||
        document.activeElement !== el ||
        typeof before !== 'string' ||
        typeof after !== 'string'
      )
        return
      const at = mapCaret(before, after, el.selectionStart ?? before.length)
      void tick().then(() => el.setSelectionRange(at, at))
    })
  })

  function oninput(e: Event): void {
    binding?.set((e.currentTarget as HTMLInputElement).value, policy)
  }
  function onfocus(): void {
    binding?.setFocused(true)
    view.fire(node, path, 'focusChange', scope, { focused: true })
  }
  function onblur(): void {
    touched = true
    binding?.setFocused(false)
    void binding?.commit()
    view.fire(node, path, 'focusChange', scope, { focused: false })
  }
  async function onkeydown(e: KeyboardEvent): Promise<void> {
    if (e.key !== 'Enter' || e.isComposing) return
    touched = true
    await binding?.commit()
    view.fire(node, path, 'submit', scope, { value })
  }
</script>

<div
  class="field"
  class:invalid={!!error}
  class:disabled={common.disabled}
  style={common.style}
  title={common.help}
>
  {#if node.slots?.prefix?.length}<span class="affix"
      ><Children nodes={node.slots.prefix} {path} slotName="prefix" {scope} /></span
    >{/if}
  <TextInput
    bind:ref={input}
    testid={common.testid}
    {value}
    {placeholder}
    type={kind}
    readonly={readOnly}
    disabled={common.disabled}
    maxlength={rules.maxLength}
    required={rules.required}
    enterkeyhint={submitLabel === 'enter' ? undefined : submitLabel}
    autocapitalize={oneOf(
      p('autocapitalize'),
      ['none', 'sentences', 'words', 'characters'] as const,
      'sentences'
    )}
    autocorrect={p('autocorrect') === false ? 'off' : undefined}
    aria-label={name}
    {...common.hint ? { 'aria-description': common.hint } : {}}
    aria-invalid={error ? 'true' : undefined}
    aria-describedby={error ? errorId : undefined}
    {oninput}
    {onfocus}
    {onblur}
    {onkeydown}
    oncompositionstart={() => binding?.setComposing(true)}
    oncompositionend={() => binding?.setComposing(false)}
  />
  <span class="status" aria-live="polite">{binding?.dirty ? 'Saving…' : ''}</span>
  {#if node.slots?.suffix?.length}<span class="affix"
      ><Children nodes={node.slots.suffix} {path} slotName="suffix" {scope} /></span
    >{/if}
</div>
{#if error}<p class="error" id={errorId} role="alert">{error}</p>{/if}

<style>
  .field {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 12px;
    background: var(--fill-tertiary);
    border-radius: var(--radius-md);
  }
  .field:focus-within {
    outline: 2px solid var(--tint);
    outline-offset: -2px;
  }
  .invalid {
    outline: 2px solid var(--ios-red);
    outline-offset: -2px;
  }
  .disabled {
    opacity: 0.5;
  }
  .affix {
    display: flex;
    align-items: center;
    color: var(--label-secondary);
  }
  .status {
    font: var(--type-caption1);
    color: var(--label); /* secondary label on the field fill is 4.45:1: just under AA */
    white-space: nowrap;
    min-width: 0;
  }
  .status:empty {
    display: none;
  }
  .error {
    margin: 4px 12px 0;
    font: var(--type-footnote);
    color: var(--ios-red);
  }
</style>
