<!-- @molecule ConfirmAlert — the second look before something is removed, as an alert over the window: what goes and what stays, Cancel (the safe answer, in front) and the red answer that goes ahead -->
<script lang="ts">
  interface Props {
    /** The question: `Remove web?` */
    heading: string
    /** What removing it does, and what it leaves. */
    message: string
    /** The word on the button that goes ahead. */
    action?: string
    /** The alert's test id; the buttons' are this with `-cancel` and `-ok`. */
    testid: string
    onconfirm: () => void
    oncancel: () => void
  }

  let { heading, message, action = 'Remove', testid, onconfirm, oncancel }: Props = $props()

  let dialog = $state<HTMLDialogElement>()
  let cancel = $state<HTMLElement>()

  // A modal dialog: the window behind it is dimmed and inert, the focus stays inside, and it starts on the safe
  // answer, so a stray Return removes nothing.
  $effect(() => {
    dialog?.showModal()
    // the kit's button draws its own button a moment after it is made: focus it once it has
    const button = cancel as (HTMLElement & { updateComplete?: Promise<unknown> }) | undefined
    void Promise.resolve(button?.updateComplete).then(() => button?.focus())
    return () => dialog?.close()
  })
</script>

<!-- Escape answers Cancel: the key itself, and the browser's own request to close a modal dialog (a back gesture) -->
<dialog
  bind:this={dialog}
  class="alert"
  role="alertdialog"
  aria-labelledby="{testid}-heading"
  aria-describedby="{testid}-message"
  onkeydown={(e) => {
    if (e.key !== 'Escape') return
    e.preventDefault()
    oncancel()
  }}
  oncancel={(e) => {
    e.preventDefault()
    oncancel()
  }}
  data-testid={testid}
>
  <div class="text">
    <h2 id="{testid}-heading">{heading}</h2>
    <p id="{testid}-message">{message}</p>
  </div>
  <div class="answers">
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button bind:this={cancel} class="safe" variant="plain" size="fill" onclick={oncancel} data-testid="{testid}-cancel">Cancel</ui-button>
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="plain" size="fill" destructive onclick={onconfirm} data-testid="{testid}-ok">{action}</ui-button>
  </div>
</dialog>

<style>
  .alert {
    box-sizing: border-box;
    width: min(270px, calc(100vw - 32px));
    padding: 0;
    border: 0;
    border-radius: var(--radius-xl);
    background: var(--bg-elevated);
    color: var(--label);
    box-shadow: var(--shadow-popover);
    overflow: hidden;
  }
  .alert::backdrop {
    background: var(--dimming);
  }
  .text {
    padding: 20px 16px 16px;
    text-align: center;
  }
  h2 {
    margin: 0;
    font: var(--type-headline);
    overflow-wrap: anywhere;
  }
  p {
    margin: 4px 0 0;
    font: var(--type-footnote);
  }
  /* the two answers share the width, with a hairline over them and between them */
  .answers {
    display: grid;
    grid-template-columns: 1fr 1fr;
    border-top: 1px solid var(--separator);
  }
  /* each answer fills its half, square: the alert's own corners round them */
  .answers :global(ui-button::part(button)) {
    border-radius: 0;
    font-weight: 400;
  }
  .answers :global(ui-button + ui-button) {
    border-inline-start: 1px solid var(--separator);
  }
  /* the answer that changes nothing is the one in front */
  .answers :global(.safe::part(button)) {
    font-weight: 600;
  }
</style>
