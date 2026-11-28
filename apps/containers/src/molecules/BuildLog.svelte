<!-- @molecule BuildLog — how a recipe's image stands (being built, built, failed, out of date or never built) and the lines of the build this window saw, as they come, kept scrolled to the newest -->
<script lang="ts">
  import type { RecipeStatus } from '../lib/recipes.svelte'

  interface Props {
    status: RecipeStatus
    /** The lines of the last build this window heard of; none for none. */
    lines: readonly string[]
    /** Its test id; the status line's and the log's follow from it (`-status`, `-lines`). */
    testid: string
  }

  let { status, lines, testid }: Props = $props()

  let log = $state<HTMLElement>()
  // each line moves the newest one: keep it in view
  $effect(() => {
    void lines.length
    log?.scrollTo({ top: log.scrollHeight })
  })
</script>

<div class="build">
  <p class="status {status.tone}" role="status" data-testid="{testid}-status">
    {#if status.tone === 'running'}<ui-spinner label="Building"></ui-spinner>{/if}
    <span>{status.text}</span>
  </p>
  {#if lines.length > 0}
    <!-- a region that scrolls is reached with the keyboard -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <pre bind:this={log} tabindex="0" aria-label="Build log" data-testid="{testid}-lines">{lines.join('\n')}</pre>
  {/if}
</div>

<style>
  .build {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 4px 0;
  }
  .status {
    margin: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    overflow-wrap: anywhere;
    font: var(--type-subheadline);
    color: var(--label-secondary);
  }
  .done {
    color: var(--label);
  }
  .failed {
    color: var(--ios-red);
  }
  pre {
    margin: 0;
    padding: 8px 12px;
    max-height: 220px;
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    border-radius: var(--radius-sm);
    background: var(--fill-quaternary);
    color: var(--label);
    font: var(--type-footnote);
    font-family: var(--font-mono);
  }
</style>
