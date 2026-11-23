<!-- @organism ContainerLog — the end of what a container has written, or why there is nothing to read -->
<script lang="ts">
  import type { Logs } from '../types'

  interface Props {
    /** There is no container, so no log: `none` says so. */
    absent: boolean
    /** Why there is no container and when there will be one, in the app's words. */
    none: string
    /** The log, once it is read. */
    logs: Logs | null
    /** The app's word, which the test ids start with: `dns` gives `dns-log`, `dns-log-none`, `dns-log-loading`, `dns-log-empty` and `dns-log-truncated`. */
    testid: string
  }

  let { absent, none, logs, testid }: Props = $props()
</script>

<section class="log" aria-label="Log">
  {#if absent}
    <ui-empty-state
      icon="doc"
      heading="No Log"
      message={none}
      data-testid="{testid}-log-none"
    ></ui-empty-state>
  {:else if logs === null}
    <div class="waiting"><ui-spinner label="Reading the log" data-testid="{testid}-log-loading"></ui-spinner></div>
  {:else if logs.text === ''}
    <ui-empty-state icon="doc" heading="Nothing Written Yet" message="The server has written nothing so far." data-testid="{testid}-log-empty"
    ></ui-empty-state>
  {:else}
    {#if logs.truncated}
      <p class="cut" data-testid="{testid}-log-truncated">The end of a longer log.</p>
    {/if}
    <!-- a region that scrolls is reached with the keyboard -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <pre tabindex="0" data-testid="{testid}-log">{logs.text}</pre>
  {/if}
</section>

<style>
  .log {
    box-sizing: border-box;
    height: 100%;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 4px 16px 16px;
  }
  .waiting {
    flex: 1;
    display: grid;
    place-items: center;
  }
  .cut {
    margin: 0 0 4px;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  pre {
    flex: 1;
    min-height: 0;
    margin: 0;
    padding: 10px 12px;
    overflow: auto;
    border-radius: var(--radius-lg);
    background: var(--bg-secondary);
    font: var(--type-footnote);
    font-family: var(--font-mono);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  pre:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: -2px;
  }
</style>
