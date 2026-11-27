<!-- @molecule ActivityCard — one thing an agent did in a chat that is not its words: its reasoning, a tool call (what it changed, where, what came of it) or its plan -->
<script lang="ts">
  type Item = VosApp.Agents.Item
  import { opens, planOf, pretty, toolOf, type Piece } from '../lib/items'

  interface Props {
    /** A `reasoning`, `tool` or `plan` item. */
    item: Item
  }

  let { item }: Props = $props()

  const tool = $derived(item.kind === 'tool' ? toolOf(item) : null)
  const plan = $derived(item.kind === 'plan' ? planOf(item) : [])
  const STATUS: Record<string, string> = { pending: 'waiting', in_progress: 'running', completed: 'done', failed: 'failed' }
  const STEP: Record<string, string> = { pending: 'To do', in_progress: 'Doing', completed: 'Done' }
  const title = $derived(item.kind === 'reasoning' ? 'Reasoning' : item.kind === 'plan' ? 'Plan' : item.name || 'A tool')
  // a diff's lines, each with what it is: added, removed, a hunk's head or unchanged
  const lines = (patch: string): { text: string; is: 'add' | 'del' | 'hunk' | 'same' }[] =>
    patch
      .split('\n')
      .filter((line, i, all) => !(line === '' && i === all.length - 1) && !line.startsWith('--- ') && !line.startsWith('+++ '))
      .map((text) => ({ text, is: text.startsWith('+') ? 'add' : text.startsWith('-') ? 'del' : text.startsWith('@@') ? 'hunk' : 'same' }))
</script>

{#snippet piece(p: Piece, at: number)}
  {#if p.type === 'text'}
    <pre>{p.text}</pre>
  {:else if p.type === 'diff'}
    <figure class="diff" data-testid="agents-item-{item.id}-diff-{at}">
      <figcaption>{p.new_file ? 'New file' : 'Changed'}: {p.path}</figcaption>
      <pre>{#each lines(p.patch) as line, n (n)}<span class={line.is}>{line.text}</span>{/each}</pre>
    </figure>
  {:else if p.type === 'image'}
    <img src="data:{p.mime_type};base64,{p.data}" alt="What the tool produced ({p.mime_type})" />
  {:else if p.type === 'link'}
    <p class="note">
      {#if opens(p.uri)}<a href={p.uri} target="_blank" rel="noreferrer" data-testid="agents-item-{item.id}-link-{at}">{p.name}</a
        >{:else}{p.name}{/if}
      {#if p.name !== p.uri}<span class="uri">{p.uri}</span>{/if}
    </p>
  {:else if p.type === 'resource'}
    <p class="note">{p.uri}</p>
    <pre>{p.text}</pre>
  {:else}
    <p class="note">A terminal of the agent's ({p.terminal_id})</p>
  {/if}
{/snippet}

<div class="card {item.kind}" data-testid="agents-item-{item.kind}-{item.id}">
  <!-- what an agent thought and what a tool did are there to be read, not in the way: closed until asked for; a plan is
       what the agent is doing, and open -->
  <details open={item.kind === 'plan'}>
    <summary data-testid="agents-item-{item.kind}-{item.id}-summary">
      <span class="title">{title}</span>
      {#if tool?.kind}<span class="tag">{tool.kind}</span>{/if}
      {#if !item.done}<ui-spinner label="In progress"></ui-spinner>{/if}
      {#if tool?.status}<span class={tool.status === 'failed' ? 'bad' : 'note'}>{STATUS[tool.status]}</span>{/if}
    </summary>
    {#if item.kind === 'reasoning'}
      <p class="said">{item.text}</p>
    {:else if item.kind === 'plan'}
      <ol class="plan">
        {#each plan as entry, i (i)}
          <li class={entry.status}>
            <span class="step">{STEP[entry.status] ?? entry.status}</span>
            <span class="what">{entry.content}</span>
            {#if entry.priority === 'high'}<span class="tag">high priority</span>{/if}
          </li>
        {/each}
      </ol>
    {:else if tool}
      {#each tool.locations as location, i (i)}
        <p class="note">{location.path}{location.line !== undefined ? `:${location.line}` : ''}</p>
      {/each}
      {#each tool.content as p, i (i)}
        {@render piece(p, i)}
      {/each}
      {#if tool.input !== undefined}
        <p class="note">Given</p>
        <pre>{pretty(tool.input)}</pre>
      {/if}
      {#if tool.output !== undefined}
        <p class="note">Answered</p>
        <pre>{pretty(tool.output)}</pre>
      {/if}
    {/if}
  </details>
</div>

<style>
  .card {
    align-self: flex-start;
    box-sizing: border-box;
    max-width: min(100%, 72ch);
    min-width: 0;
    padding: 8px 12px;
    border-radius: var(--radius-lg);
    border: 1px solid var(--separator);
    background: var(--bg-secondary);
    font: var(--type-footnote);
  }
  summary {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    cursor: pointer;
  }
  .title {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .tag {
    padding: 0 6px;
    border-radius: var(--radius-sm, 4px);
    background: var(--fill-tertiary);
    /* the secondary label is too faint on the fill */
    color: var(--label);
  }
  .note {
    margin: 6px 0 0;
    color: var(--label-secondary);
    overflow-wrap: anywhere;
  }
  .uri {
    margin-inline-start: 6px;
  }
  .bad {
    margin: 0;
    color: var(--ios-red);
  }
  .said {
    margin: 6px 0 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: var(--label-secondary);
  }
  pre {
    margin: 4px 0 0;
    max-height: 240px;
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-family: ui-monospace, monospace;
  }
  .diff {
    margin: 6px 0 0;
  }
  .diff figcaption {
    color: var(--label-secondary);
    overflow-wrap: anywhere;
  }
  /* a line each; what was added and removed is tinted behind the text, which keeps the text's own contrast */
  .diff span {
    display: block;
    min-height: 1lh;
  }
  .diff .add {
    background: color-mix(in srgb, var(--ios-green) 20%, transparent);
  }
  .diff .del {
    background: color-mix(in srgb, var(--ios-red) 20%, transparent);
  }
  .diff .hunk {
    color: var(--label-secondary);
  }
  img {
    display: block;
    max-width: 100%;
    margin-top: 6px;
    border-radius: var(--radius-sm, 4px);
  }
  a {
    color: inherit;
    text-decoration: underline;
  }
  .plan {
    margin: 6px 0 0;
    padding-inline-start: 20px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .plan li {
    overflow-wrap: anywhere;
  }
  .step {
    color: var(--label-secondary);
    margin-inline-end: 6px;
  }
  .plan .completed .what {
    text-decoration: line-through;
    color: var(--label-secondary);
  }
</style>
