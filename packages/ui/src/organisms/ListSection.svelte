<!-- @organism ListSection — inset-grouped / plain / sidebar list section. Composes ListRow molecules via children. -->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    header?: string
    footer?: string
    /** insetGrouped (Settings) or plain (edge-to-edge) */
    style?: 'insetGrouped' | 'plain'
    /** Sidebar style — transparent rows on a grouped background, used in split views. */
    sidebar?: boolean
    children: Snippet
  }

  let { header, footer, style = 'insetGrouped', sidebar = false, children }: Props = $props()
</script>

<section class="ios-list-section {style}" class:sidebar>
  {#if header}<h3 class="header">{header}</h3>{/if}
  <div class="rows">
    {@render children()}
  </div>
  {#if footer}<p class="footer">{footer}</p>{/if}
</section>

<style>
  .ios-list-section {
    margin: 0 0 35px;
  }
  .insetGrouped {
    padding: 0 var(--list-inset, 20px);
  }
  .header {
    margin: 0;
    padding: 0 var(--row-inset) 7px;
    font: var(--type-footnote);
    letter-spacing: var(--tracking-footnote);
    font-weight: 400;
    text-transform: uppercase;
    color: var(--label-secondary);
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
  .insetGrouped .rows {
    border-radius: var(--radius-md);
  }
  .plain .rows {
    background: var(--bg-elevated);
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
  }
</style>
