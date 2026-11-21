<!--
  @atom Markdown — Markdown rendered as elements, never as markup. The source is parsed to a closed tree of blocks
  and inline nodes (`../markdown`, the `marked` lexer) and each node becomes a Svelte element; raw HTML in the
  source is shown as text, images are never fetched, and a link keeps its target only for http, https and mailto.
  Clicking a link (or middle-clicking it, which would otherwise open a tab on its own) calls `onlink(url)` (the
  host opens it, after the usual confirmation) and never navigates the window itself. Test ids: `testid` on the root, `${testid}-link-<n>` on each link (in document order).
-->
<script lang="ts">
  import { parseMarkdown, type Block, type Inline, type MarkdownOptions } from '../markdown'

  interface Props {
    testid: string
    text: string
    /** Enabled features; default all of tables, tasklist, code. */
    options?: MarkdownOptions
    /** Let the person select and copy the text. */
    selectable?: boolean
    onlink?: (url: string) => void
  }

  let { testid, text, options, selectable = false, onlink }: Props = $props()

  const blocks = $derived(parseMarkdown(text, options))

  // Link numbering restarts with every render of the tree: ids are by order of appearance.
  const linkIds = $derived.by(() => {
    const ids = new Map<Inline, number>()
    let n = 0
    const walkInline = (nodes: Inline[]): void => {
      for (const node of nodes) {
        if (node.t === 'link') ids.set(node, n++)
        if ('children' in node) walkInline(node.children)
      }
    }
    const walk = (list: Block[]): void => {
      for (const b of list) {
        if (b.t === 'heading' || b.t === 'paragraph') walkInline(b.children)
        else if (b.t === 'quote') walk(b.children)
        else if (b.t === 'list') for (const item of b.items) walk(item.children)
        else if (b.t === 'table') {
          for (const cell of b.header) walkInline(cell)
          for (const row of b.rows) for (const cell of row) walkInline(cell)
        }
      }
    }
    walk(blocks)
    return ids
  })

  function open(e: MouseEvent, href: string): void {
    e.preventDefault()
    onlink?.(href)
  }
  /** Middle-click opens a tab straight from the `href`: take it over, so it asks like a click does. */
  function openAux(e: MouseEvent, href: string): void {
    if (e.button === 1) open(e, href)
  }
  const textAlign = (a: 'left' | 'center' | 'right' | null): string | undefined => a ?? undefined
</script>

{#snippet inlines(nodes: Inline[])}
  {#each nodes as node, i (i)}
    {#if node.t === 'text'}{node.text}
    {:else if node.t === 'strong'}<strong>{@render inlines(node.children)}</strong>
    {:else if node.t === 'em'}<em>{@render inlines(node.children)}</em>
    {:else if node.t === 'del'}<del>{@render inlines(node.children)}</del>
    {:else if node.t === 'code'}<code>{node.text}</code>
    {:else if node.t === 'br'}<br />
    {:else if node.t === 'link'}<a
        href={node.href}
        rel="noopener noreferrer"
        data-testid="{testid}-link-{linkIds.get(node) ?? 0}"
        onclick={(e) => open(e, node.href)}
        onauxclick={(e) => openAux(e, node.href)}>{@render inlines(node.children)}</a
      >
    {/if}
  {/each}
{/snippet}

{#snippet flow(list: Block[])}
  {#each list as block, i (i)}
    {#if block.t === 'heading'}
      <svelte:element this={`h${block.depth}`} class="h h{block.depth}">{@render inlines(block.children)}</svelte:element>
    {:else if block.t === 'paragraph'}
      <p>{@render inlines(block.children)}</p>
    {:else if block.t === 'code'}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex (a scrollable region must be focusable to scroll by keyboard) -->
      <pre tabindex="0"><code>{block.text}</code></pre>
    {:else if block.t === 'quote'}
      <blockquote>{@render flow(block.children)}</blockquote>
    {:else if block.t === 'list'}
      <svelte:element this={block.ordered ? 'ol' : 'ul'} start={block.ordered ? block.start : undefined} class:tasks={block.items.some((x) => x.checked !== undefined)}>
        {#each block.items as item, j (j)}
          <li class:task={item.checked !== undefined}>
            {#if item.checked !== undefined}
              <span class="box" class:checked={item.checked} role="img" aria-label={item.checked ? 'Done' : 'Not done'}>
                {#if item.checked}<svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7" /></svg>{/if}
              </span>
            {/if}
            <div class="item">{@render flow(item.children)}</div>
          </li>
        {/each}
      </svelte:element>
    {:else if block.t === 'table'}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex (a scrollable region must be focusable to scroll by keyboard) -->
      <div class="table-scroll" tabindex="0" role="region" aria-label="Table">
        <table>
          <thead>
            <tr>
              {#each block.header as cell, c (c)}
                <th scope="col" style:text-align={textAlign(block.align[c] ?? null)}>{@render inlines(cell)}</th>
              {/each}
            </tr>
          </thead>
          <tbody>
            {#each block.rows as row, r (r)}
              <tr>
                {#each row as cell, c (c)}
                  <td style:text-align={textAlign(block.align[c] ?? null)}>{@render inlines(cell)}</td>
                {/each}
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else if block.t === 'hr'}
      <hr />
    {/if}
  {/each}
{/snippet}

<div class="ios-markdown" class:selectable data-testid={testid}>
  {@render flow(blocks)}
</div>

<style>
  .ios-markdown {
    min-width: 0;
    font: var(--type-body);
    letter-spacing: var(--tracking-body);
    color: var(--label);
    overflow-wrap: anywhere;
  }
  .selectable {
    -webkit-user-select: text;
    user-select: text;
  }
  .ios-markdown > :global(:first-child) {
    margin-top: 0;
  }
  .ios-markdown > :global(:last-child) {
    margin-bottom: 0;
  }
  p,
  ul,
  ol,
  blockquote,
  pre,
  .table-scroll {
    margin: 0 0 12px;
  }
  .h {
    margin: 16px 0 6px;
    font-weight: 600;
    line-height: 1.25;
  }
  .h1 {
    font: var(--type-title2);
    font-weight: 700;
    letter-spacing: var(--tracking-title2);
  }
  .h2 {
    font: var(--type-title3);
    font-weight: 600;
    letter-spacing: var(--tracking-title3);
  }
  .h3 {
    font: var(--type-headline);
    letter-spacing: var(--tracking-body);
  }
  .h4,
  .h5,
  .h6 {
    font: var(--type-subheadline);
    font-weight: 600;
    letter-spacing: var(--tracking-subheadline);
  }
  .h6 {
    color: var(--label-secondary);
  }
  a {
    color: var(--link);
    text-decoration: underline;
    text-underline-offset: 2px;
    cursor: pointer;
  }
  code {
    font-family: var(--font-mono);
    font-size: 0.92em;
    padding: 1px 5px;
    border-radius: var(--radius-xs);
    background: var(--fill-tertiary);
  }
  pre {
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    background: var(--fill-tertiary);
    overflow: auto;
    max-width: 100%;
  }
  pre code {
    padding: 0;
    background: none;
    font-size: 0.88em;
    /* Code wraps rather than scrolling sideways (a window is never wider than its content: DESIGN_LANGUAGE 8). */
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  blockquote {
    padding-inline-start: 12px;
    border-inline-start: 3px solid var(--separator-opaque);
    color: var(--label-secondary);
  }
  ul,
  ol {
    padding-inline-start: 24px;
  }
  .tasks {
    list-style: none;
    padding-inline-start: 0;
  }
  li {
    margin: 2px 0;
  }
  li.task {
    display: flex;
    gap: 8px;
    align-items: flex-start;
  }
  .item :global(p) {
    margin-bottom: 4px;
  }
  .box {
    flex: none;
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    margin-top: 2px;
    border-radius: 50%;
    border: 2px solid var(--label-secondary);
    color: transparent;
  }
  .box.checked {
    border-color: transparent;
    background: var(--tint-fill);
    color: #fff;
  }
  .table-scroll {
    overflow: auto;
    max-width: 100%;
  }
  table {
    border-collapse: collapse;
    font: var(--type-subheadline);
    width: 100%;
  }
  th,
  td {
    overflow-wrap: anywhere;
    padding: 6px 10px;
    border: var(--hairline) solid var(--separator);
    text-align: start;
    vertical-align: top;
  }
  th {
    font-weight: 600;
    background: var(--fill-quaternary);
  }
  hr {
    border: 0;
    border-top: var(--hairline) solid var(--separator);
    margin: 14px 0;
  }
</style>
