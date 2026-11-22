<!-- @molecule ItemForm — the next item as it is typed, as a form of iPadOS is: a row each for what the password is for, whose it is and the password itself; what the keychain refused in the footer under the rows; and an Add button that cannot be pressed until there is something to add -->
<script lang="ts">
  import type { ItemDraft } from '../lib/keychain.svelte'

  interface Props {
    /** What is typed so far. */
    draft: ItemDraft
    /** There is something to add: a service and an account. */
    ready: boolean
    /** The backend's word on the item that was sent and not added (the keychain has a password for it already). */
    refused?: string | null
    /** The item is being added: the machine may be asking its person. */
    busy?: boolean
    ondraft: (change: Partial<ItemDraft>) => void
    onadd: () => void
  }

  let { draft, ready, refused = null, busy = false, ondraft, onadd }: Props = $props()

  const valueOf = (e: Event): string => (e.currentTarget as unknown as { value: string }).value
</script>

<form
  class="item"
  novalidate
  onsubmit={(e) => {
    e.preventDefault()
    if (ready && !busy) onadd()
  }}
>
  <ui-list variant="inset" label="A new item">
    <ui-section
      footer={refused ?? 'Leave the password empty for an item the keychain has already.'}
      footer-tone={refused ? 'error' : 'note'}
      data-testid="keychain-new-form"
    >
      <ui-row>
        <span class="key">Service</span>
        <ui-text-field
          variant="bare"
          label="Service"
          placeholder="router.home"
          autocomplete="off"
          autocapitalize="off"
          enterkeyhint="next"
          maxlength={200}
          value={draft.service}
          oninput={(e) => ondraft({ service: valueOf(e) })}
          data-testid="keychain-new-service"
        ></ui-text-field>
      </ui-row>
      <ui-row>
        <span class="key">Account</span>
        <ui-text-field
          variant="bare"
          label="Account"
          placeholder="admin"
          autocomplete="off"
          autocapitalize="off"
          enterkeyhint="next"
          maxlength={200}
          value={draft.account}
          oninput={(e) => ondraft({ account: valueOf(e) })}
          data-testid="keychain-new-account"
        ></ui-text-field>
      </ui-row>
      <ui-row>
        <span class="key">Password</span>
        <ui-text-field
          variant="bare"
          type="password"
          label="Password"
          description={refused ?? undefined}
          placeholder="Optional"
          autocomplete="new-password"
          autocapitalize="off"
          enterkeyhint="done"
          maxlength={4000}
          invalid={refused !== null}
          value={draft.password}
          oninput={(e) => ondraft({ password: valueOf(e) })}
          data-testid="keychain-new-password"
        ></ui-text-field>
      </ui-row>
    </ui-section>
  </ui-list>
  <div class="buttons">
    {#if busy}<ui-spinner label="Adding the item" data-testid="keychain-new-busy"></ui-spinner>{/if}
    <ui-button type="submit" hit disabled={!ready || busy} data-testid="keychain-new-add">Add</ui-button>
  </div>
</form>

<style>
  .item {
    display: flex;
    flex-direction: column;
    gap: 8px;
    /* the rows line up with the rest of the window, and the button sits close under their footer */
    --list-inset: 16px;
  }
  ui-section {
    margin-bottom: 0;
  }
  .key {
    flex: none;
    min-width: 84px;
  }
  .buttons {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    padding-inline: 16px;
  }
</style>
