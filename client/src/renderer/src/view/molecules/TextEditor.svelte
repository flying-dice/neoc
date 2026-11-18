<!--
  @molecule TextEditor — a multi-line editor with a two-way text binding and drafts. What is typed shows at once
  and is written back debounced (300 ms, at most 1 s of continuous typing between writes); an agent's append
  while the person is typing is held back, then merged on the host and adopted with the caret kept in place.
  Enter inserts a new line. `font = mono` for code, `readOnly`, `spellcheck`; `selection` is an out-binding of
  `{from, to}` reported as the selection changes. A "Saving…" status is announced politely. Atoms: TextArea.
-->
<script lang="ts">
  import { TextArea } from '@agentos/ui/atoms'
  import { useView } from '../context'
  import { commonOf, str } from '../props'
  import type { AdapterProps } from '../registry'
  import { appearanceStyle } from '../kit/support'
  import { textControl } from '../kit/textControl.svelte'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  let area: HTMLTextAreaElement | undefined = $state()
  const control = textControl({
    ctx,
    node: () => node,
    path: () => path,
    scope: () => scope,
    element: () => area
  })

  const common = $derived(commonOf(ctx, node, path, scope))
  const placeholder = $derived(str(p('placeholder')))
  const name = $derived(common.label ?? (placeholder || undefined))
  const readOnly = $derived(p('readOnly') === true || control.readOnly)
  const selection = $derived(view.bindingOf(node.props?.selection, scope))
  const appearance = $derived(appearanceStyle(ctx, node, scope))

  /** Report the caret or selection as `{from, to}` (a client-state or state binding; no write if unbound). */
  function reportSelection(): void {
    if (!selection || !area) return
    const from = area.selectionStart ?? 0
    const to = area.selectionEnd ?? from
    const current = selection.value as { from?: number; to?: number } | null
    if (current && current.from === from && current.to === to) return
    selection.set({ from, to }, { mode: 'change' })
  }
</script>

<div class="editor" class:disabled={common.disabled} style="{common.style};{appearance}" title={common.help}>
  <TextArea
    bind:ref={area}
    testid={common.testid}
    value={control.value}
    {placeholder}
    mono={p('font') === 'mono'}
    readonly={readOnly}
    disabled={common.disabled}
    spellcheck={p('spellcheck') === false ? false : undefined}
    aria-label={name}
    aria-description={common.hint}
    aria-invalid={control.error ? 'true' : undefined}
    oninput={(e) => {
      control.oninput(e)
      reportSelection()
    }}
    onfocus={control.onfocus}
    onblur={control.onblur}
    onselect={reportSelection}
    onkeyup={reportSelection}
    onpointerup={reportSelection}
    oncompositionstart={control.oncompositionstart}
    oncompositionend={control.oncompositionend}
  />
  <span class="status" aria-live="polite">{control.saving ? 'Saving…' : ''}</span>
</div>
{#if control.error}<p class="error" role="alert">{control.error}</p>{/if}

<style>
  .editor {
    position: relative;
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 96px;
    padding: 10px 12px;
    background: var(--fill-tertiary);
    border-radius: var(--radius-md);
  }
  .editor:focus-within {
    outline: 2px solid var(--tint);
    outline-offset: -2px;
  }
  .disabled {
    opacity: 0.5;
  }
  .status {
    align-self: flex-end;
    font: var(--type-caption1);
    color: var(--label);
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
