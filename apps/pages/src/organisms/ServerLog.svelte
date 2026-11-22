<!-- @organism ServerLog — the end of what the server has written: how it started, what it could not do, and every request it answered -->
<script lang="ts">
  import type { Logs, Server } from 'app:rpc'

  interface Props {
    server: Server
    /** The log, once it is read. */
    logs: Logs | null
  }

  let { server, logs }: Props = $props()
</script>

<section class="log" aria-label="Log">
  {#if server.state === 'absent'}
    <ui-empty-state
      icon="doc"
      heading="No Log"
      message="The server has no container. It writes here once a site is on."
      data-testid="pages-log-none"
    ></ui-empty-state>
  {:else if logs === null}
    <div class="waiting"><ui-spinner label="Reading the log" data-testid="pages-log-loading"></ui-spinner></div>
  {:else if logs.text === ''}
    <ui-empty-state icon="doc" heading="Nothing Written Yet" message="The server has written nothing so far." data-testid="pages-log-empty"
    ></ui-empty-state>
  {:else}
    {#if logs.truncated}
      <p class="cut" data-testid="pages-log-truncated">The end of a longer log.</p>
    {/if}
    <!-- a region that scrolls is reached with the keyboard -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <pre tabindex="0" data-testid="pages-log">{logs.text}</pre>
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
