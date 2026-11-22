<!-- @page KeychainPage — the whole app: the items of this machine's keychain and the form that adds one, the question before one is deleted, or why there is no keychain to show; driven by one window's Keychain state -->
<script lang="ts">
  import ConfirmAlert from '../molecules/ConfirmAlert.svelte'
  import ItemList from '../organisms/ItemList.svelte'
  import type { Keychain } from '../lib/keychain.svelte'

  interface Props {
    /** This window's state; the composition root builds it with the real `api` and `host`, a story with a fake. */
    keychain: Keychain
  }

  let { keychain }: Props = $props()

  $effect(() => {
    void keychain.load()
    return keychain.listen()
  })
</script>

<main class="app" data-testid="keychain-app">
  {#if !keychain.loaded}
    <div class="waiting">
      {#if !keychain.failure}<ui-spinner size="large" label="Opening the keychain" data-testid="keychain-loading"></ui-spinner>{/if}
    </div>
  {:else if !keychain.store.available}
    <div class="waiting">
      <ui-empty-state icon="lock" heading="No Keychain" message={keychain.store.problem} data-testid="keychain-unavailable"></ui-empty-state>
    </div>
  {:else}
    <div class="rest">
      <ItemList
        items={keychain.items}
        draft={keychain.draft}
        ready={keychain.ready}
        refused={keychain.refused}
        adding={keychain.adding}
        password={(item) => keychain.password(item)}
        problem={(item) => keychain.problem(item)}
        busy={(item) => keychain.busy(item)}
        ondraft={(change) => keychain.edit(change)}
        onadd={() => void keychain.add()}
        onreveal={(item) => void keychain.reveal(item)}
        onhide={(item) => keychain.hide(item)}
        onremove={(item) => keychain.ask(item)}
      />
    </div>
  {/if}
  {#if keychain.asking}
    <ConfirmAlert
      heading="Delete {keychain.asking.service}?"
      message="The password of {keychain.asking.account} is deleted from the keychain of this machine."
      testid="keychain-confirm"
      onconfirm={() => void keychain.remove()}
      oncancel={() => keychain.keep()}
    />
  {/if}
  <!-- why the last call failed (the host's sentence), whatever is in front; what the keychain refused is said at the form or the item -->
  {#if keychain.failure}
    <ui-banner tone="error" data-testid="keychain-failure">{keychain.failure}</ui-banner>
  {/if}
</main>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg);
    color: var(--label);
    font: var(--type-body);
  }
  .rest {
    flex: 1;
    min-height: 0;
  }
  .waiting {
    flex: 1;
    display: grid;
    place-items: center;
    padding: 16px;
  }
</style>
