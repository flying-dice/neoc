<!-- @atom ToolStep — one step of an agent's turn that is not its words, as a quiet line: a tool call (an icon for its kind and its title) or its reasoning; opened in place, what the tool was given, produced and changed, or what the agent thought -->
<script lang="ts">
  import type { Item } from '../types'
  import { opens, pretty, stepIconOf, titleOf, toolOf, type Piece } from '../lib/items'

  interface Props {
    /** A `tool` or a `reasoning` item. */
    item: Item
    /** Open a web address a tool gave outside the app. */
    onlink: (url: string) => void
    /** The app's word, which the test ids start with: `agents` gives `agents-item-tool-3`. */
    testid: string
  }

  let { item, onlink, testid }: Props = $props()

  const tool = $derived(item.kind === 'tool' ? toolOf(item) : null)
  const title = $derived(titleOf(item))
  const icon = $derived(stepIconOf(item))
  const failed = $derived(tool?.status === 'failed')
  // what it did is drawn once it is opened: a closed step is its line alone
  let open = $state(false)
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
    <figure class="diff" data-testid="{testid}-item-{item.id}-diff-{at}">
      <figcaption>{p.new_file ? 'New file' : 'Changed'}: {p.path}</figcaption>
      <pre>{#each lines(p.patch) as line, n (n)}<span class={line.is}>{line.text}</span>{/each}</pre>
    </figure>
  {:else if p.type === 'image'}
    <img src="data:{p.mime_type};base64,{p.data}" alt="What the tool produced ({p.mime_type})" />
  {:else if p.type === 'link'}
    <p class="note">
      {#if opens(p.uri)}<a
          href={p.uri}
          target="_blank"
          rel="noopener noreferrer"
          onclick={(e) => {
            // the frame cannot open a tab of its own: the host opens it
            e.preventDefault()
            onlink(p.uri)
          }}
          data-testid="{testid}-item-{item.id}-link-{at}">{p.name}</a
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

<!-- what a tool did and what the agent thought are there to be read, not in the way: a line, closed until asked for -->
<details class="step" bind:open data-testid="{testid}-item-{item.kind}-{item.id}">
  <summary data-testid="{testid}-item-{item.kind}-{item.id}-summary">
    <ui-icon class="kind" name={icon} size={17}></ui-icon>
    <span class="title">{title}</span>
    {#if failed}<span class="bad">· failed</span>{:else if !item.done}<span class="hidden">(running)</span>{/if}
    <ui-icon class="more" name="chevron.right" size={13} weight={2.5}></ui-icon>
  </summary>
  {#if open}
    <div class="details">
      {#if item.kind === 'reasoning'}
        <p class="said">{item.text}</p>
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
        {#if tool.locations.length === 0 && tool.content.length === 0 && tool.input === undefined && tool.output === undefined}
          <p class="note">Nothing more to show.</p>
        {/if}
      {/if}
    </div>
  {/if}
</details>

<style>
  .step {
    align-self: stretch;
    min-width: 0;
    font: var(--type-subheadline);
    color: var(--label-secondary);
  }
  summary {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 28px;
    padding: 2px 0;
    list-style: none;
    cursor: pointer;
    border-radius: var(--radius-sm);
  }
  summary::-webkit-details-marker {
    display: none;
  }
  summary:hover .title {
    color: var(--label);
  }
  .title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .more {
    transition: transform var(--duration-fast, 0.15s) var(--ease-ios, ease);
  }
  .step[open] .more {
    transform: rotate(90deg);
  }
  /* a quiet word in the error tint after the title */
  .bad {
    flex: none;
    color: var(--ios-red);
  }
  .hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  /* what it did, under the line and set in from it */
  .details {
    margin: 2px 0 6px 9px;
    padding: 2px 0 2px 17px;
    border-inline-start: 1px solid var(--separator);
    font: var(--type-footnote);
  }
  .note {
    margin: 6px 0 0;
    overflow-wrap: anywhere;
  }
  .details > :first-child {
    margin-top: 0;
  }
  .uri {
    margin-inline-start: 6px;
  }
  .said {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  pre {
    margin: 4px 0 0;
    padding: 6px 8px;
    max-height: 240px;
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    border-radius: var(--radius-sm);
    background: var(--fill-quaternary);
    color: var(--label);
    font-family: var(--font-mono);
  }
  .diff {
    margin: 6px 0 0;
  }
  .diff figcaption {
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
    border-radius: var(--radius-sm);
  }
  a {
    color: var(--link);
    text-decoration: underline;
  }
</style>
