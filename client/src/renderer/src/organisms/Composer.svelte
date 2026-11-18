<!--
  @organism Composer — chat input bar with an optional accessory row above it
  (e.g. session controls). Atoms: Material, TextInput. Molecules: IconButton.
  Test ids: `testid` on the form, `${testid}-input`, `${testid}-send`, `${testid}-attach`.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import { Material, TextInput } from '@agentos/ui/atoms'
  import { IconButton } from '@agentos/ui/molecules'

  interface Props {
    testid: string
    value?: string
    placeholder?: string
    disabled?: boolean
    accessory?: Snippet
    onsend: (text: string) => void
  }

  let {
    testid,
    value = $bindable(''),
    placeholder = 'Message',
    disabled = false,
    accessory,
    onsend
  }: Props = $props()

  let input: HTMLInputElement | undefined = $state()

  /** Focus the text field, caret at the end. */
  export function focus(): void {
    input?.focus()
    input?.setSelectionRange(value.length, value.length)
  }

  function send(): void {
    if (!value.trim() || disabled) return
    onsend(value.trim())
    value = ''
  }

  function submit(e: SubmitEvent): void {
    e.preventDefault()
    send()
  }
</script>

<Material thickness="chrome">
  <div class="surface">
    {#if accessory}{@render accessory()}{/if}
    <form class="composer" data-testid={testid} onsubmit={submit}>
      <IconButton
        testid="{testid}-attach"
        icon="plus"
        label="Attach"
        variant="gray"
        size="small"
        capsule
      />
      <label class="field">
        <TextInput
          testid="{testid}-input"
          bind:ref={input}
          bind:value
          {placeholder}
          aria-label={placeholder}
        />
      </label>
      <IconButton
        testid="{testid}-send"
        icon="arrow.up"
        label="Send"
        variant="filled"
        size="small"
        capsule
        disabled={!value.trim() || disabled}
        onclick={send}
      />
    </form>
  </div>
</Material>

<style>
  .surface {
    box-shadow: 0 calc(var(--hairline) * -1) 0 var(--separator);
  }
  .composer {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 12px 8px;
  }
  .field {
    flex: 1;
    display: flex;
    min-height: 34px;
    align-items: center;
    padding: 0 14px;
    border-radius: 17px;
    box-shadow: inset 0 0 0 1px var(--separator-opaque);
    background: var(--bg-elevated);
  }
</style>
