<!--
  @organism ListSection — a group of rows with an optional header and footer, in one of the five list styles
  (plain, inset, insetGrouped, grouped, sidebar). Composes ListRow molecules via children. Molecules: SectionHeader.
  Test ids: `testid` (optional) on the section, `${testid}-header` on a collapsible header button.

  Inside a `List` it takes the list's style and semantics: its rows are a `list` of `listitem`s (or a `group` of
  `option`s in a selectable list) labelled by the header. Alone it is the plain container the shell uses.
  `collapsible` turns the header into a disclosure button (`aria-expanded`); the rows are removed while collapsed.
-->
<script lang="ts">
  import { getContext, type Snippet } from 'svelte'
  import SectionHeader from '../molecules/SectionHeader.svelte'
  import { LIST_CONTEXT, type ListStyle, type ListStyleContext } from '../logic/listContext'

  interface Props {
    testid?: string
    header?: string
    footer?: string
    /** The list style when used alone; inside a `List` the list's style wins. */
    style?: ListStyle
    /** Sidebar style — transparent rows on a grouped background, used in split views. */
    sidebar?: boolean
    collapsible?: boolean
    expanded?: boolean
    onexpandedchange?: (expanded: boolean) => void
    children: Snippet
  }

  let {
    testid,
    header,
    footer,
    style = 'insetGrouped',
    sidebar = false,
    collapsible = false,
    expanded = $bindable(true),
    onexpandedchange,
    children
  }: Props = $props()

  const list = getContext<ListStyleContext | undefined>(LIST_CONTEXT)
  const kind = $derived<ListStyle>(list ? list.style : sidebar ? 'sidebar' : style)
  const uid = $props.id()
  const semantic = $derived(list !== undefined)
  const rowsRole = $derived(semantic ? (list?.selectable ? 'group' : 'list') : undefined)

  function toggle(): void {
    expanded = !expanded
    onexpandedchange?.(expanded)
  }
</script>

<section class="ios-list-section {kind}" class:sidebar={kind === 'sidebar'} data-testid={testid}>
  {#if header}
    {#if collapsible}
      <h3 class="header collapsible" id="{uid}-header" role={list?.selectable ? 'presentation' : undefined}>
        <SectionHeader testid="{testid ?? uid}-header" title={header} {expanded} ontoggle={toggle} />
      </h3>
    {:else}
      <h3 class="header" id="{uid}-header" role={list?.selectable ? 'presentation' : undefined}>{header}</h3>
    {/if}
  {/if}
  {#if expanded}
    <div class="rows" role={rowsRole} aria-labelledby={semantic && header ? `${uid}-header` : undefined}>
      {@render children()}
    </div>
  {/if}
  {#if footer}<p class="footer" role={list?.selectable ? 'presentation' : undefined}>{footer}</p>{/if}
</section>

<style>
  .ios-list-section {
    margin: 0 0 35px;
    min-width: 0;
  }
  .header {
    margin: 0;
    padding: 0 var(--row-inset) 7px;
    font: var(--type-footnote);
    letter-spacing: var(--tracking-footnote);
    font-weight: 400;
    text-transform: uppercase;
    color: var(--label-secondary);
    overflow-wrap: anywhere;
  }
  .header.collapsible {
    padding: 0;
  }
  .footer {
    margin: 0;
    padding: 7px var(--row-inset) 0;
    font: var(--type-footnote);
    letter-spacing: var(--tracking-footnote);
    color: var(--label-secondary);
  }
  .rows {
    overflow: hidden;
    background: var(--list-row-bg, var(--bg-grouped-elevated-secondary));
  }

  /* insetGrouped (Settings): rounded blocks inset from the edges. */
  .insetGrouped {
    padding: 0 var(--list-inset, 20px);
  }
  .insetGrouped .rows {
    border-radius: var(--radius-md);
  }

  /* inset: rounded blocks inset from the edges, on the primary background. */
  .inset {
    padding: 0 var(--list-inset, 20px);
    margin-bottom: 24px;
    --list-row-bg: var(--bg-secondary);
  }
  .inset .rows {
    border-radius: var(--radius-md);
  }

  /* plain: edge to edge, on the primary background. */
  .plain {
    margin-bottom: 0;
    --list-row-bg: var(--bg-elevated);
  }
  .plain .rows {
    background: var(--bg-elevated);
  }
  .plain .header {
    padding-top: 12px;
    padding-bottom: 6px;
    background: var(--bg-grouped-elevated);
  }

  /* grouped: edge to edge blocks on the grouped background. */
  .grouped .rows {
    background: var(--list-row-bg, var(--bg-grouped-elevated-secondary));
  }

  /* Sidebar: rows float on the grouped background, selection is a filled pill. */
  .sidebar {
    margin-bottom: 22px;
    padding: 0 10px;
  }
  .sidebar .header {
    padding-inline: 10px;
    font: var(--type-headline);
    text-transform: none;
    color: var(--label);
  }
  .sidebar .rows {
    background: transparent;
    overflow: visible;
    --list-separator: transparent;
    --list-row-radius: var(--radius-md);
    --list-row-bg: transparent;
  }
</style>
