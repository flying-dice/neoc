<!-- @molecule ItemRow — one item in the list: what the password is for, whose it is, the password once the person asks to see it (or why it cannot be shown), and the buttons that show or hide it and delete the item -->
<script lang="ts">
  import type { Item } from 'app:rpc'

  interface Props {
    item: Item
    /** The password, while it is shown. */
    password?: string | null
    /** Why the password could not be shown, or the item not deleted. */
    problem?: string | null
    /** A call is in flight for it: the buttons give way to a spinner. */
    busy?: boolean
    onreveal: () => void
    onhide: () => void
    onremove: () => void
  }

  let { item, password = null, problem = null, busy = false, onreveal, onhide, onremove }: Props = $props()

  const id = $derived(`keychain-item-${item.id}`)
  const shown = $derived(password !== null)
</script>

<li class="row" data-testid={id}>
  <span class="text">
    <span class="service" data-testid="{id}-service">{item.service}</span>
    <span class="account" data-testid="{id}-account">{item.account}</span>
    {#if password !== null}
      <span class="password" data-testid="{id}-password">{password}</span>
    {:else if problem !== null}
      <span class="problem" role="alert" data-testid="{id}-problem">{problem}</span>
    {/if}
  </span>
  <span class="act">
    {#if busy}
      <ui-spinner label="Asking the keychain for {item.service}" data-testid="{id}-busy"></ui-spinner>
    {:else}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button
        variant="plain"
        hit
        label="{shown ? 'Hide' : 'Show'} the password of {item.account} for {item.service}"
        onclick={shown ? onhide : onreveal}
        data-testid="{id}-{shown ? 'hide' : 'reveal'}"
      >
        {shown ? 'Hide' : 'Show'}
      </ui-button>
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button
        variant="plain"
        square
        destructive
        hit
        label="Delete {item.account} for {item.service}"
        onclick={onremove}
        data-testid="{id}-remove"
      >
        <ui-icon slot="icon" name="trash"></ui-icon>
      </ui-button>
    {/if}
  </span>
</li>

<style>
  .row {
    list-style: none;
    display: flex;
    align-items: center;
    gap: 8px;
    padding-inline-start: 12px;
    border-bottom: 1px solid var(--separator);
  }
  .row:last-child {
    border-bottom: 0;
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 6px 0;
  }
  .service,
  .account {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .service {
    font: var(--type-body);
    font-weight: 600;
  }
  .account {
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  /* a password is read character by character, and copied: every one of them is there, on as many lines as it takes */
  .password {
    font: var(--type-footnote);
    font-family: var(--font-mono);
    overflow-wrap: anywhere;
    user-select: all;
  }
  .problem {
    font: var(--type-footnote);
    color: var(--ios-red);
  }
  /* the buttons and the spinner that stands in for them take the same room */
  .act {
    flex: none;
    min-width: 44px;
    min-height: 44px;
    display: flex;
    align-items: center;
    justify-content: flex-end;
  }
</style>
