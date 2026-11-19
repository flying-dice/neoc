<!--
  @molecule SecureField — password input with a reveal toggle. Like a TextField (draft first, caret kept) but the
  value is written back on submit (Enter) or blur by default: a half-typed secret is not sent mid-keystroke. The
  reveal button is a toggle button (`aria-pressed`) with its own test id `<id>-reveal`. Atoms: TextInput, Button,
  Icon.
-->
<script lang="ts">
  import { Button, Icon, TextInput } from '@agentos/ui/atoms'
  import { useView } from '../context'
  import { commonOf, str } from '../props'
  import type { AdapterProps } from '../registry'
  import { appearanceStyle } from '../kit/support'
  import { textControl } from '../kit/textControl.svelte'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  let input: HTMLInputElement | undefined = $state()
  let revealed = $state(false)
  const control = textControl({
    ctx,
    node: () => node,
    path: () => path,
    scope: () => scope,
    element: () => input,
    defaultSync: { mode: 'submit' },
    submitOnEnter: true
  })

  const common = $derived(commonOf(ctx, node, path, scope))
  const placeholder = $derived(str(p('placeholder')))
  const name = $derived(common.label ?? (placeholder || undefined))
  const appearance = $derived(appearanceStyle(ctx, node, scope))
</script>

<div
  class="field"
  class:disabled={common.disabled}
  style="{common.style};{appearance}"
  title={common.help}
>
  <TextInput
    bind:ref={input}
    testid={common.testid}
    value={control.value}
    {placeholder}
    type={revealed ? 'text' : 'password'}
    readonly={control.readOnly}
    disabled={common.disabled}
    autocomplete="off"
    autocapitalize="none"
    spellcheck={false}
    aria-label={name}
    aria-description={common.hint}
    oninput={control.oninput}
    onfocus={control.onfocus}
    onblur={control.onblur}
    onkeydown={(e) => void control.onkeydown(e)}
    oncompositionstart={control.oncompositionstart}
    oncompositionend={control.oncompositionend}
  />
  <Button
    testid="{common.testid}-reveal"
    variant="plain"
    size="small"
    square
    disabled={common.disabled}
    aria-label={revealed ? 'Hide password' : 'Show password'}
    aria-pressed={revealed}
    onclick={() => (revealed = !revealed)}
  >
    <Icon name={revealed ? 'eye.slash' : 'eye'} size={18} weight={2} />
  </Button>
</div>
{#if control.error}<p class="error" role="alert">{control.error}</p>{/if}

<style>
  .field {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 4px 0 12px;
    background: var(--fill-tertiary);
    border-radius: var(--radius-md);
  }
  .field:focus-within {
    outline: 3px solid var(--focus-ring);
    outline-offset: -2px;
  }
  .disabled {
    opacity: 0.5;
  }
  .error {
    margin: 4px 12px 0;
    font: var(--type-footnote);
    color: var(--ios-red);
  }
</style>
