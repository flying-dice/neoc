<!-- @molecule ConnectionForm — which Docker the server runs in: the address of one elsewhere, or empty for this machine's own -->
<script lang="ts">
  interface Props {
    /** What is typed so far. */
    draft: string
    /** The address that is chosen now; empty for this machine's own. */
    host: string
    ondraft: (address: string) => void
    onconnect: () => void
  }

  let { draft, host, ondraft, onconnect }: Props = $props()

  // nothing to do while what is typed is what is chosen already; the button then tries the same Docker again
  const same = $derived(draft.trim() === host)
</script>

<form
  class="connection"
  onsubmit={(e) => {
    e.preventDefault()
    onconnect()
  }}
>
  <ui-text-field
    label="Docker address"
    description="Empty for this machine's own Docker"
    placeholder="This machine's Docker"
    inputmode="url"
    autocomplete="off"
    autocapitalize="off"
    enterkeyhint="go"
    maxlength={300}
    hit
    value={draft}
    oninput={(e) => ondraft((e.currentTarget as unknown as { value: string }).value)}
    data-testid="dns-host-input"
  ></ui-text-field>
  <ui-button type="submit" variant="tinted" hit data-testid="dns-host-connect">{same ? 'Try Again' : 'Connect'}</ui-button>
</form>

<style>
  .connection {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  ui-text-field {
    flex: 1;
    min-width: 0;
  }
</style>
