<!-- @molecule SectionBar — the top of a list of the sidebar's: its name and a summary, the way back to the sidebar in a narrow window, and the buttons that act on the whole list -->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    title: string
    summary: string
    /** Show the way back to the sidebar (a window too narrow for both). */
    back?: boolean
    onback?: () => void
    /** The buttons at the end. */
    children?: Snippet
  }

  let { title, summary, back = false, onback, children }: Props = $props()
</script>

<header class="bar">
  <div class="lead">
    {#if back}
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="plain" size="small" hit label="Back to the sidebar" onclick={onback} data-testid="containers-back">
      <ui-icon slot="icon" name="chevron.left"></ui-icon>
      Back
    </ui-button>
    {/if}
  </div>
  <div class="heading">
    <h1 data-testid="containers-title">{title}</h1>
    <p class="summary" data-testid="containers-summary">{summary}</p>
  </div>
  <div class="trail">{@render children?.()}</div>
</header>

<style>
  /* the title is centred in the bar, the way back before it and the buttons after it */
  .bar {
    display: grid;
    grid-template-columns: minmax(max-content, 1fr) minmax(0, 2fr) minmax(max-content, 1fr);
    align-items: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 16px 8px;
  }
  .lead {
    display: flex;
    justify-content: flex-start;
  }
  .trail {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 4px;
  }
  .heading {
    min-width: 0;
    text-align: center;
  }
  h1 {
    margin: 0;
    font: var(--type-headline);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .summary {
    margin: 0;
    font: var(--type-footnote);
    color: var(--label-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
