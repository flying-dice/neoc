<!-- @organism ItemList — the items of the keychain the app shows: the form that adds one, and each item with the buttons that show its password and delete it -->
<script lang="ts">
  type Item = VosApp.Keychain.Item
  import ItemForm from '../molecules/ItemForm.svelte'
  import ItemRow from '../molecules/ItemRow.svelte'
  import { itemsSummary } from '../lib/format'
  import type { ItemDraft } from '../lib/keychain.svelte'

  interface Props {
    items: readonly Item[]
    /** The item that is being typed. */
    draft: ItemDraft
    /** There is something to add. */
    ready: boolean
    /** The backend's word on the item that was sent and not added. */
    refused?: string | null
    /** The item that is typed is being added. */
    adding?: boolean
    /** The password of an item, while it is shown. */
    password: (item: Item) => string | null
    /** Why the password of an item could not be shown. */
    problem: (item: Item) => string | null
    /** Whether a call is in flight for an item. */
    busy: (item: Item) => boolean
    ondraft: (change: Partial<ItemDraft>) => void
    onadd: () => void
    onreveal: (item: Item) => void
    onhide: (item: Item) => void
    onremove: (item: Item) => void
  }

  let { items, draft, ready, refused = null, adding = false, password, problem, busy, ondraft, onadd, onreveal, onhide, onremove }: Props = $props()
</script>

<section class="items" aria-label="Items">
  <ItemForm {draft} {ready} {refused} busy={adding} {ondraft} {onadd} />
  <p class="summary" data-testid="keychain-items-summary">{itemsSummary(items.length)}</p>
  <div class="list-box">
    {#if items.length > 0}
      <ul class="list" data-testid="keychain-items">
        {#each items as item (item.id)}
          <ItemRow
            {item}
            password={password(item)}
            problem={problem(item)}
            busy={busy(item)}
            onreveal={() => onreveal(item)}
            onhide={() => onhide(item)}
            onremove={() => onremove(item)}
          />
        {/each}
      </ul>
    {:else}
      <ui-empty-state
        icon="lock"
        heading="No Items"
        message="Keep a password in the keychain of this machine above, or name one it has already to show it here."
        data-testid="keychain-empty"
      ></ui-empty-state>
    {/if}
  </div>
</section>

<style>
  /* the form and the items scroll as one: in a short window the form is most of what fits */
  .items {
    box-sizing: border-box;
    height: 100%;
    overflow-y: auto;
    padding-top: 12px;
  }
  .summary {
    margin: 0;
    padding: 8px 16px 4px;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  .list-box {
    padding: 0 16px 16px;
  }
  .list {
    margin: 0;
    padding: 0;
    border-radius: var(--radius-lg);
    background: var(--bg-secondary);
  }
</style>
