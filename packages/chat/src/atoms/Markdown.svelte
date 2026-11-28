<!-- @atom Markdown — text the agent wrote, drawn as Markdown (headings, lists, code, tables, quotes, links), with a link out sent to the host to open -->
<script lang="ts">
  import { opens } from '../lib/items'
  import { markdownOf } from '../lib/markdown'

  interface Props {
    /** The Markdown, as much of it as has arrived. */
    text: string
    /** Open a web address outside the app (the frame cannot open a tab of its own). */
    onlink: (url: string) => void
    /** Its test id. */
    testid: string
  }

  let { text, onlink, testid }: Props = $props()

  // micromark's safe mode: the agent's HTML is text, and an address that could run code is dropped (lib/markdown.ts)
  const html = $derived(markdownOf(text))

  // a link out goes to the host, which opens it in a tab of its own; any other address (one micromark dropped, a
  // mailto) goes nowhere, and a footnote's is followed in the page
  function follow(event: MouseEvent): void {
    const link = (event.target as Element | null)?.closest('a')
    const href = link?.getAttribute('href')
    if (!link || href === null || href === undefined || href.startsWith('#')) return
    event.preventDefault()
    if (opens(href)) onlink(href)
  }
</script>

<!-- the click is a link's, which has its own role and keys (Enter on a link is a click), and bubbles to here -->
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="markdown" onclick={follow} data-testid={testid}>
  <!-- markdownOf escapes every tag of the agent's own: what is left is micromark's markup -->
  {@html html}
</div>

<style>
  .markdown {
    min-width: 0;
    overflow-wrap: anywhere;
    user-select: text;
  }
  .markdown > :global(:first-child) {
    margin-top: 0;
  }
  .markdown > :global(:last-child) {
    margin-bottom: 0;
  }
  .markdown :global(:is(p, ul, ol, blockquote, pre, .table)) {
    margin: 0 0 12px;
  }
  .markdown :global(:is(h1, h2, h3, h4, h5, h6)) {
    margin: 18px 0 6px;
    font: var(--type-headline);
  }
  .markdown :global(h1) {
    font: var(--type-title3);
    font-weight: 600;
  }
  .markdown :global(:is(strong, b)) {
    font-weight: 600;
  }
  .markdown :global(:is(ul, ol)) {
    padding-inline-start: 22px;
  }
  .markdown :global(li) {
    margin: 3px 0;
  }
  .markdown :global(li > :is(ul, ol)) {
    margin: 3px 0 0;
  }
  .markdown :global(li > p) {
    margin: 0 0 6px;
  }
  /* a task list item has its box in place of the bullet */
  .markdown :global(li:has(> input[type='checkbox'])) {
    list-style: none;
    margin-inline-start: -20px;
  }
  .markdown :global(li > input[type='checkbox']) {
    margin: 0 6px 0 0;
    accent-color: var(--tint);
    vertical-align: middle;
  }
  .markdown :global(blockquote) {
    padding: 0 0 0 12px;
    border-inline-start: 3px solid var(--separator-opaque);
    color: var(--label-secondary);
  }
  .markdown :global(code) {
    font: var(--type-subheadline);
    font-family: var(--font-mono);
    padding: 2px 4px;
    border-radius: var(--radius-xs);
    background: var(--fill-tertiary);
  }
  .markdown :global(pre) {
    padding: 10px 12px;
    border-radius: var(--radius-md);
    background: var(--fill-quaternary);
    border: 1px solid var(--separator);
    overflow-x: auto;
    font: var(--type-footnote);
  }
  .markdown :global(pre code) {
    padding: 0;
    border-radius: 0;
    background: none;
    font: inherit;
    font-family: var(--font-mono);
    white-space: pre;
    overflow-wrap: normal;
  }
  .markdown :global(.table) {
    overflow-x: auto;
  }
  .markdown :global(table) {
    border-collapse: collapse;
    font: var(--type-subheadline);
  }
  .markdown :global(:is(th, td)) {
    padding: 8px 12px;
    border: 1px solid var(--separator);
    text-align: start;
    vertical-align: top;
    overflow-wrap: normal;
  }
  .markdown :global(th) {
    font-weight: 600;
    background: var(--fill-quaternary);
  }
  .markdown :global(a) {
    color: var(--link);
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .markdown :global(a:not([href^='http'], [href^='#'])) {
    color: inherit;
  }
  .markdown :global(hr) {
    margin: 16px 0;
    border: 0;
    border-top: 1px solid var(--separator);
  }
  .markdown :global(img) {
    max-width: 100%;
  }
  .markdown :global(.sr-only) {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  .markdown :global(.footnotes) {
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
</style>
