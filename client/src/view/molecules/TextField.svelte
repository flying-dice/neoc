<!--
  @molecule TextField — the two-way text binding in the UI. The input shows the effective value (the user's draft
  first, then the server value) and writes every keystroke into the view's draft overlay; the core debounces the
  write-back, shields the field from remote changes while dirty, and adopts the host's merge on the ack, mapping
  the caret through the diff. Composition (IME) never flushes mid-composition; blur and Enter commit. With
  `lines` above 1 it is a multi-line field. `label`, `hint`, `disabled`, `kind`, `autocapitalize`, `autocorrect`,
  `submitLabel` pass through to the input; `validate` shows an inline error after the first blur. Atoms:
  TextInput, TextArea, Children.
-->
<script lang="ts">
  import { TextArea, TextInput } from '@vos/ui/atoms'
  import Children from '../atoms/Children.svelte'
  import { parseRules, validateText } from '../core/validate'
  import { useView } from '../context'
  import { commonOf, oneOf, str } from '../props'
  import type { AdapterProps } from '../registry'
  import { appearanceStyle } from '../kit/support'
  import { textControl } from '../kit/textControl.svelte'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  const KINDS = ['text', 'email', 'url', 'number', 'tel', 'search'] as const
  const SUBMIT = ['enter', 'done', 'go', 'next', 'search', 'send'] as const

  let input: HTMLInputElement | undefined = $state()
  let area: HTMLTextAreaElement | undefined = $state()
  const control = textControl({
    ctx,
    node: () => node,
    path: () => path,
    scope: () => scope,
    element: () => input ?? area,
    submitOnEnter: true
  })

  const common = $derived(commonOf(ctx, node, path, scope))
  const placeholder = $derived(str(p('placeholder')))
  const kind = $derived(oneOf(p('kind'), KINDS, 'text'))
  const lines = $derived(Math.min(12, Math.max(1, Math.round(Number(p('lines')) || 1))))
  const name = $derived(common.label ?? (placeholder || undefined))
  const rules = $derived(parseRules(p('validate')))
  const submitLabel = $derived(oneOf(p('submitLabel'), SUBMIT, 'enter'))
  const appearance = $derived(appearanceStyle(ctx, node, scope))

  let touched = $state(false)
  const problem = $derived(touched ? validateText(control.value, rules, kind === 'number') : null)
  const error = $derived(control.error ?? problem ?? undefined)
  const errorId = $derived(`${common.testid}-error`)
  const autocapitalize = $derived(
    oneOf(p('autocapitalize'), ['none', 'sentences', 'words', 'characters'] as const, 'sentences')
  )
</script>

<div
  class="field"
  class:invalid={!!error}
  class:disabled={common.disabled}
  class:multi={lines > 1}
  style="{common.style};{appearance}"
  title={common.help}
>
  {#if node.slots?.prefix?.length}<span class="affix"
      ><Children nodes={node.slots.prefix} {path} slotName="prefix" {scope} /></span
    >{/if}
  {#if lines > 1}
    <TextArea
      bind:ref={area}
      testid={common.testid}
      value={control.value}
      {placeholder}
      rows={lines}
      readonly={control.readOnly}
      disabled={common.disabled}
      maxlength={rules.maxLength}
      required={rules.required}
      {autocapitalize}
      spellcheck={p('autocorrect') === false ? false : undefined}
      aria-label={name}
      aria-description={common.hint}
      aria-invalid={error ? 'true' : undefined}
      aria-describedby={error ? errorId : undefined}
      oninput={control.oninput}
      onfocus={control.onfocus}
      onblur={() => {
        touched = true
        control.onblur()
      }}
      oncompositionstart={control.oncompositionstart}
      oncompositionend={control.oncompositionend}
    />
  {:else}
    <TextInput
      bind:ref={input}
      testid={common.testid}
      value={control.value}
      {placeholder}
      type={kind}
      readonly={control.readOnly}
      disabled={common.disabled}
      maxlength={rules.maxLength}
      required={rules.required}
      enterkeyhint={submitLabel === 'enter' ? undefined : submitLabel}
      {autocapitalize}
      autocorrect={p('autocorrect') === false ? 'off' : undefined}
      aria-label={name}
      aria-description={common.hint}
      aria-invalid={error ? 'true' : undefined}
      aria-describedby={error ? errorId : undefined}
      oninput={control.oninput}
      onfocus={control.onfocus}
      onblur={() => {
        touched = true
        control.onblur()
      }}
      onkeydown={(e) => {
        if (e.key === 'Enter') touched = true
        void control.onkeydown(e)
      }}
      oncompositionstart={control.oncompositionstart}
      oncompositionend={control.oncompositionend}
    />
  {/if}
  <span class="status" aria-live="polite">{control.saving ? 'Saving…' : ''}</span>
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
  .field.multi {
    align-items: flex-start;
    padding-block: 10px;
  }
  .field:focus-within {
    outline: 3px solid var(--focus-ring);
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
