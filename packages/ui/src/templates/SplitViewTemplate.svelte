<!--
  @template SplitViewTemplate — UISplitViewController: sidebar + detail, or sidebar + content + detail (the iPadOS
  Settings / Mail layout). Below `collapseAt` it collapses to ONE column, `compactColumn`, which the caller moves
  (a navigation flow: sidebar → content → detail). With three columns, a narrow regular window hides the sidebar
  first (`visibility` `double`); `detail` shows the detail alone. Columns are `aside`/`section` landmarks-in-waiting:
  the content of each is the caller's. Test ids: `${testid}-sidebar`, `-content`, `-detail` when `testid` is set.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'

  type Column = 'sidebar' | 'content' | 'detail'

  interface Props {
    testid?: string
    sidebarWidth?: number
    contentWidth?: number
    /** Container width (px) below which the split collapses to one column. */
    collapseAt?: number
    /** Below this width a three-column split shows `double` instead of `all`. */
    doubleAt?: number
    visibility?: 'all' | 'double' | 'detail'
    /** The column shown while collapsed. */
    compactColumn?: Column
    /** Narrower sidebar and content, so the detail has the room. */
    prominentDetail?: boolean
    /** Keep hidden columns mounted (scroll and state survive a collapse); they are inert and invisible. */
    keepMounted?: boolean
    /** Out: whether the split is collapsed to one column right now. */
    collapsed?: boolean
    /** Out: the visibility actually in effect (after `doubleAt`). */
    effective?: 'all' | 'double' | 'detail'
    sidebar: Snippet
    content?: Snippet
    detail: Snippet
  }

  let {
    testid,
    sidebarWidth = 320,
    contentWidth = 320,
    collapseAt = 560,
    doubleAt = 900,
    visibility = 'all',
    compactColumn = 'detail',
    prominentDetail = false,
    keepMounted = false,
    collapsed = $bindable(false),
    effective = $bindable('all'),
    sidebar,
    content,
    detail
  }: Props = $props()

  let width = $state(0)
  $effect(() => {
    collapsed = width > 0 && width < collapseAt
  })
  const narrow = $derived(content !== undefined && visibility === 'all' && width > 0 && width < doubleAt)
  const mode = $derived<'all' | 'double' | 'detail'>(narrow ? 'double' : visibility)
  $effect(() => {
    effective = mode
  })

  const shown = $derived<Record<Column, boolean>>(
    collapsed
      ? { sidebar: compactColumn === 'sidebar', content: compactColumn === 'content', detail: compactColumn === 'detail' }
      : {
          sidebar: mode === 'all',
          content: content !== undefined && mode !== 'detail',
          detail: true
        }
  )
  const factor = $derived(prominentDetail ? 0.85 : 1)
</script>

<div
  class="split"
  class:collapsed
  bind:clientWidth={width}
  style:--sidebar-width="{Math.round(sidebarWidth * factor)}px"
  style:--content-width="{Math.round(contentWidth * factor)}px"
>
  {#if keepMounted || shown.sidebar}
    <aside class="sidebar" class:off={!shown.sidebar} inert={!shown.sidebar} data-testid={testid ? `${testid}-sidebar` : undefined}>
      {@render sidebar()}
    </aside>
  {/if}
  {#if content && (keepMounted || shown.content)}
    <section class="content" class:off={!shown.content} inert={!shown.content} data-testid={testid ? `${testid}-content` : undefined}>
      {@render content()}
    </section>
  {/if}
  {#if keepMounted || shown.detail}
    <section class="detail" class:off={!shown.detail} inert={!shown.detail} data-testid={testid ? `${testid}-detail` : undefined}>
      {@render detail()}
    </section>
  {/if}
</div>

<style>
  .split {
    position: relative;
    display: flex;
    width: 100%;
    height: 100%;
    min-height: 0;
  }
  .sidebar,
  .content,
  .detail {
    min-width: 0;
    overflow-y: auto;
    background: var(--bg-grouped-elevated);
    --list-row-bg: var(--bg-grouped-elevated-secondary);
  }
  .sidebar {
    flex: none;
    width: min(var(--sidebar-width), 40%);
    box-shadow: var(--hairline) 0 0 var(--separator);
    z-index: 2;
  }
  .content {
    flex: none;
    width: min(var(--content-width), 38%);
    box-shadow: var(--hairline) 0 0 var(--separator);
    z-index: 1;
  }
  .detail {
    flex: 1;
  }
  /* Collapsed: one column fills the split; the others wait invisible (scroll kept) or are not mounted. */
  .collapsed .sidebar,
  .collapsed .content,
  .collapsed .detail {
    position: absolute;
    inset: 0;
    width: auto;
    box-shadow: none;
  }
  .off {
    visibility: hidden;
    pointer-events: none;
  }
  /* Hidden but mounted, wide: out of the flow. */
  .split:not(.collapsed) .off {
    display: none;
  }
</style>
