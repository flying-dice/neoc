<!-- @molecule Markdown — a note as it reads: its Markdown drawn as headings, paragraphs, lists with tasks that can be ticked, quotes and code, with its links to other notes, its links out and its tags as things to press -->
<script lang="ts">
  import { parse, type List, type Run, type Style } from '../lib/markdown'

  interface Props {
    /** The note's Markdown. */
    body: string
    /** The test id of the view; a link's is this with `-link-<title>`, a tag's with `-tag-<tag>`, a task's with `-task-<line>`. */
    testid: string
    /** Whether a link's title names a note: a link to no note is drawn fainter, and makes the note when it is followed. */
    resolves?: (title: string) => boolean
    /** A link to a note was pressed: its title. */
    onfollow: (title: string) => void
    /** A link out of the app was pressed: its address. */
    onlink: (url: string) => void
    /** A tag was pressed: the tag, without the `#`. */
    ontag: (tag: string) => void
    /** A task was ticked or unticked: the line of the text it is on, from 0. */
    ontick: (line: number) => void
  }

  let { body, testid, resolves = () => true, onfollow, onlink, ontag, ontick }: Props = $props()

  const blocks = $derived(parse(body))

  const classOf = (style: Style): string =>
    [style.bold && 'bold', style.italic && 'italic', style.strike && 'strike', style.mark && 'mark'].filter(Boolean).join(' ')
</script>

{#snippet runs(line: readonly Run[])}
  {#each line as run, i (i)}
    {#if run.kind === 'text'}
      <span class={classOf(run.style)}>{run.text}</span>
    {:else if run.kind === 'code'}
      <code>{run.text}</code>
    {:else if run.kind === 'wiki'}
      <button
        type="button"
        class="link {classOf(run.style)}"
        class:missing={!resolves(run.target)}
        title={resolves(run.target) ? run.target : `${run.target} is not written yet`}
        onclick={() => onfollow(run.target)}
        data-testid="{testid}-link-{run.target}">{run.text}</button
      >
    {:else if run.kind === 'link'}
      <button type="button" class="link out {classOf(run.style)}" title={run.url} onclick={() => onlink(run.url)} data-testid="{testid}-out"
        >{run.text}</button
      >
    {:else}
      <button type="button" class="tag" onclick={() => ontag(run.tag)} data-testid="{testid}-tag-{run.tag}">{run.text}</button>
    {/if}
  {/each}
{/snippet}

{#snippet lines(said: readonly (readonly Run[])[])}
  {#each said as line, i (i)}{#if i > 0}<br />{/if}{@render runs(line)}{/each}
{/snippet}

{#snippet items(list: List)}
  <svelte:element this={list.ordered ? 'ol' : 'ul'} class:tasks={list.items.some((item) => item.task !== null)}>
    {#each list.items as item (item.line)}
      <li class:task={item.task !== null} class:done={item.task === true}>
        {#if item.task !== null}
          <input
            type="checkbox"
            checked={item.task}
            aria-label={item.runs.map((run) => run.text).join('')}
            onchange={() => ontick(item.line)}
            data-testid="{testid}-task-{item.line}"
          />
        {/if}
        <span class="said">{@render runs(item.runs)}</span>
        {#each item.children as nested, i (i)}{@render items(nested)}{/each}
      </li>
    {/each}
  </svelte:element>
{/snippet}

<article class="markdown" data-testid={testid}>
  {#each blocks as block, i (i)}
    {#if block.kind === 'heading'}
      <svelte:element this={`h${block.level}`}>{@render runs(block.runs)}</svelte:element>
    {:else if block.kind === 'paragraph'}
      <p>{@render lines(block.lines)}</p>
    {:else if block.kind === 'list'}
      {@render items(block.list)}
    {:else if block.kind === 'quote'}
      <blockquote>{@render lines(block.lines)}</blockquote>
    {:else if block.kind === 'code'}
      <!-- a block of code scrolls sideways, so it can be reached with the keys -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <pre tabindex="0"><code>{block.text}</code></pre>
    {:else}
      <hr />
    {/if}
  {/each}
</article>

<style>
  .markdown {
    font: var(--type-body);
    color: var(--label);
    overflow-wrap: anywhere;
    user-select: text;
  }
  .markdown > :first-child {
    margin-top: 0;
  }
  h1,
  h2,
  h3,
  h4,
  h5,
  h6 {
    margin: 20px 0 8px;
    font: var(--type-headline);
  }
  h1 {
    font: var(--type-title1);
    font-weight: 700;
  }
  h2 {
    font: var(--type-title2);
    font-weight: 700;
  }
  h3 {
    font: var(--type-title3);
    font-weight: 600;
  }
  p,
  blockquote,
  pre,
  ul,
  ol {
    margin: 0 0 12px;
  }
  ul,
  ol {
    padding-inline-start: 24px;
  }
  li {
    margin: 2px 0;
  }
  li > ul,
  li > ol {
    margin: 2px 0 0;
  }
  /* a task has its check in place of the bullet */
  .task {
    list-style: none;
    margin-inline-start: -24px;
    display: grid;
    grid-template-columns: 24px minmax(0, 1fr);
    align-items: baseline;
  }
  .task > :global(ul),
  .task > :global(ol) {
    grid-column: 2;
  }
  .task input {
    margin: 0;
    width: 16px;
    height: 16px;
    accent-color: var(--tint);
    align-self: center;
  }
  .task.done > .said {
    color: var(--label-secondary);
    text-decoration: line-through;
  }
  blockquote {
    padding: 2px 0 2px 12px;
    border-inline-start: 3px solid var(--tint);
    color: var(--label-secondary);
  }
  code {
    font-family: var(--font-mono);
    font-size: 0.9em;
    padding: 1px 4px;
    border-radius: var(--radius-xs);
    background: var(--fill-tertiary);
  }
  pre {
    padding: 10px 12px;
    border-radius: var(--radius-md);
    background: var(--fill-tertiary);
    overflow-x: auto;
  }
  pre code {
    padding: 0;
    background: none;
    white-space: pre;
  }
  hr {
    margin: 16px 0;
    border: 0;
    border-top: 1px solid var(--separator);
  }
  .bold {
    font-weight: 600;
  }
  .italic {
    font-style: italic;
  }
  .strike {
    text-decoration: line-through;
  }
  .mark {
    background: color-mix(in srgb, var(--ios-yellow) 35%, transparent);
    border-radius: 2px;
  }
  /* a link and a tag are buttons that sit in the line as words do */
  button {
    display: inline;
    margin: 0;
    padding: 0;
    border: 0;
    background: none;
    font: inherit;
    text-align: inherit;
    cursor: pointer;
  }
  button:focus-visible,
  pre:focus-visible,
  .task input:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: 1px;
    border-radius: var(--radius-xs);
  }
  .link {
    color: var(--link);
    text-decoration: underline;
    text-decoration-color: color-mix(in srgb, var(--link) 40%, transparent);
    text-underline-offset: 2px;
  }
  /* a note that is not written yet: the link is there, the note is not */
  .link.missing {
    color: var(--label-secondary);
    text-decoration-style: dashed;
    text-decoration-color: currentColor;
  }
  .tag {
    padding: 0 8px;
    border-radius: var(--radius-pill);
    background: var(--fill-tertiary);
    color: var(--link);
    font: var(--type-subheadline);
  }
</style>
