<!-- @molecule BuildLog — how a copilot's image stands: built from the Dockerfile as it is, out of date, never built or failed; and the lines of the build this window saw, as they come, kept scrolled to the newest -->
<script lang="ts">
  type Copilot = VosApp.Copilot.Copilot
  import type { Build } from '../lib/copilot.svelte'

  interface Props {
    /** The copilot as it is saved; null for one not added yet. */
    copilot: Copilot | null
    /** The last build this window heard of; null for none. */
    build: Build | null
    /** Its test id; the status line's and the log's follow from it (`-status`, `-lines`). */
    testid: string
  }

  let { copilot, build, testid }: Props = $props()

  const status = $derived.by((): { tone: 'running' | 'done' | 'failed' | 'none'; text: string } => {
    if (build?.status === 'running' || copilot?.building) return { tone: 'running', text: 'Building…' }
    if (build?.status === 'failed' || (copilot && copilot.problem !== '')) return { tone: 'failed', text: `Failed: ${copilot?.problem || build?.lines.at(-1) || ''}` }
    if (copilot && copilot.image !== '' && copilot.current) return { tone: 'done', text: `Built: ${copilot.image}` }
    if (copilot && copilot.image !== '') return { tone: 'none', text: 'The Dockerfile changed since the last build.' }
    return { tone: 'none', text: 'Not built yet. A chat builds it when it first needs it.' }
  })

  let log = $state<HTMLElement>()
  // each line moves the newest one: keep it in view
  $effect(() => {
    void build?.lines.length
    log?.scrollTo({ top: log.scrollHeight })
  })
</script>

<div class="build">
  <p class="status {status.tone}" role="status" data-testid="{testid}-status">
    {#if status.tone === 'running'}<ui-spinner label="Building"></ui-spinner>{/if}
    <span>{status.text}</span>
  </p>
  {#if build && build.lines.length > 0}
    <!-- a region that scrolls is reached with the keyboard -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <pre bind:this={log} tabindex="0" aria-label="Build log" data-testid="{testid}-lines">{build.lines.join('\n')}</pre>
  {/if}
</div>

<style>
  .build {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 6px 0;
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
    padding: 8px 10px;
    max-height: 220px;
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    border-radius: var(--radius-sm);
    background: var(--fill-quaternary);
    color: var(--label);
    font-family: var(--font-mono);
    font-size: 12px;
  }
</style>
