<!--
  @page PairingPage — shown to a browser the host does not know yet: enter the code the host printed (and
  `vos pair` prints again) to pair this device. iPadOS "Sign in" style: a card on the wallpaper. Wrong
  code, rate limiting and expiry are said plainly, once, under the field.

  Test ids: `pairing` (the page), `pairing-code`, `pairing-device-name`, `pairing-submit`, `pairing-error`.
-->
<script lang="ts">
  import { Button, Icon, IconTile, Text, TextInput } from '@vos/ui/atoms'
  import { normaliseCode, type PairResult } from '../shell/pairing'

  interface Props {
    /** Trade the code for a session. Resolves with what happened. */
    onpair: (code: string, deviceName: string) => Promise<PairResult>
    /** The host could not be reached at all (not a wrong code). */
    unreachable?: string | null
    onretry?: () => void
  }

  let { onpair, unreachable = null, onretry }: Props = $props()

  let code = $state('')
  let deviceName = $state('')
  let busy = $state(false)
  let error = $state<string | null>(null)

  const ready = $derived(code.replace(/[^A-Z0-9]/gi, '').length === 8)

  async function submit(): Promise<void> {
    if (!ready || busy) return
    busy = true
    error = null
    const result = await onpair(code, deviceName)
    busy = false
    if (!result.ok) error = result.message
  }

  function oninput(): void {
    code = normaliseCode(code)
    error = null
  }
</script>

<main class="page" data-testid="pairing">
  <form
    class="card"
    onsubmit={(e) => {
      e.preventDefault()
      void submit()
    }}
  >
    <IconTile size={72} background="linear-gradient(180deg, #5ac8fa 0%, #0a6cff 100%)" shadow>
      <Icon name="lock" size={38} weight={1.8} color="#fff" />
    </IconTile>
    <h1><Text style="title2" weight="bold" as="span">Pair this device</Text></h1>

    {#if unreachable}
      <div class="notice" role="alert" data-testid="pairing-unreachable">
        <Text style="subheadline" color="destructive">Can’t reach the vos host.</Text>
        <Text style="footnote" color="secondary">{unreachable}</Text>
        <Button testid="pairing-retry" size="small" capsule onclick={onretry}>Try again</Button>
      </div>
    {:else}
      <Text style="subheadline" color="secondary" as="p">
        Enter the pairing code shown where vos is running. It is single use and lasts 10
        minutes. Need a new one? Run <code>vos pair</code>.
      </Text>

      <label class="field">
        <Text style="footnote" color="secondary" as="span">Pairing code</Text>
        <span class="box" class:invalid={error !== null}>
          <TextInput
            testid="pairing-code"
            class="code"
            bind:value={code}
            {oninput}
            placeholder="ABCD-EFGH"
            autocomplete="off"
            autocapitalize="characters"
            spellcheck={false}
            inputmode="text"
            maxlength={12}
            aria-invalid={error !== null}
            aria-describedby={error ? 'pairing-error' : undefined}
          />
        </span>
      </label>

      <label class="field">
        <Text style="footnote" color="secondary" as="span">Name this device (optional)</Text>
        <span class="box">
          <TextInput
            testid="pairing-device-name"
            bind:value={deviceName}
            placeholder="Safari on iPad"
            autocomplete="off"
            maxlength={40}
          />
        </span>
      </label>

      {#if error}
        <p id="pairing-error" class="error" role="alert" data-testid="pairing-error">{error}</p>
      {/if}

      <Button
        testid="pairing-submit"
        type="submit"
        size="large"
        capsule
        block
        disabled={!ready || busy}
      >
        {busy ? 'Pairing…' : 'Pair'}
      </Button>
    {/if}
  </form>
</main>

<style>
  .page {
    min-height: 100vh;
    display: grid;
    place-items: center;
    padding: 24px;
    background: var(--wallpaper-default, var(--bg-grouped));
  }
  .card {
    width: min(100%, 400px);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
    padding: 32px 28px 28px;
    border-radius: var(--radius-xl);
    background: var(--bg-elevated);
    color: var(--label);
    box-shadow: var(--shadow-popover);
    text-align: center;
  }
  h1 {
    margin: 0;
  }
  .field {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 4px;
    text-align: left;
  }
  .box {
    display: flex;
    align-items: center;
    min-height: 44px;
    padding: 0 12px;
    border-radius: var(--radius-md);
    background: var(--fill-tertiary, var(--bg-grouped));
    border: var(--hairline) solid var(--separator);
  }
  .box:focus-within {
    outline: 3px solid var(--focus-ring);
    outline-offset: 1px;
  }
  .box.invalid {
    border-color: var(--ios-red);
  }
  .box :global(.code) {
    font: 600 20px/1.2 var(--font-mono, ui-monospace, monospace);
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }
  .error {
    margin: 0;
    width: 100%;
    font: var(--type-footnote);
    color: var(--ios-red);
    text-align: left;
  }
  .notice {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
  }
  code {
    font-family: var(--font-mono, ui-monospace, monospace);
  }
</style>
