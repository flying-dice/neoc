<!-- @organism PartDetail — one part of a vos app looked at: what it is for, and its facts as a grouped list of rows -->
<script lang="ts">
  import type { Part } from 'app:rpc'

  interface Props {
    part: Part
    /** Show the way back to the sidebar (a window too narrow for it). */
    back?: boolean
    onback?: () => void
  }

  let { part, back = false, onback }: Props = $props()
</script>

<section class="detail" aria-label={part.title}>
  <header class="bar">
    {#if back}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit label="Back to Example" onclick={onback} data-testid="example-detail-back">
        <ui-icon slot="icon" name="chevron.left"></ui-icon>
        Example
      </ui-button>
    {/if}
    <h1 data-testid="example-detail-title">{part.title}</h1>
  </header>
  <div class="scroll">
    <ui-list variant="inset" label={part.title}>
      <ui-section header="About" footer={part.summary} data-testid="example-detail-facts">
        {#each part.facts as fact (fact.label)}
          <ui-row heading={fact.label} value={fact.value} data-testid="example-fact-{part.id}-{fact.label.toLowerCase().replaceAll(' ', '-')}"></ui-row>
        {/each}
      </ui-section>
    </ui-list>
  </div>
</section>

<style>
  .detail {
    box-sizing: border-box;
    min-width: 0;
    height: 100%;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
    display: flex;
    flex-direction: column;
    background: var(--bg-grouped);
  }
  .bar {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    padding: 0 16px 8px;
  }
  h1 {
    margin: 0;
    font: var(--type-title2);
    font-weight: 700;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding-bottom: 16px;
  }
</style>
