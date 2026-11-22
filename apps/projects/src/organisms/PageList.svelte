<!-- @organism PageList — a project's pages, each followed by the pages under it and saying what it is under, and the field the next one is named in; a tap opens a page -->
<script lang="ts">
  import type { Page } from 'app:rpc'
  import Composer from '../molecules/Composer.svelte'
  import type { Leaf } from '../lib/catalog'

  interface Props {
    pages: readonly Leaf[]
    /** What is typed as the title of the next page. */
    draft: string
    onopen: (page: Page) => void
    ondraft: (text: string) => void
    onadd: () => void
  }

  let { pages, draft, onopen, ondraft, onadd }: Props = $props()
</script>

<section class="pages" aria-label="Pages">
  <div class="new">
    <Composer {draft} label="New Page" action="Add the page" testid="projects-new-page" {ondraft} {onadd} />
  </div>
  <div class="scroll">
    {#if pages.length > 0}
      <ui-list label="Pages" data-testid="projects-pages">
        <ui-section>
          {#each pages as leaf (leaf.page.id)}
            <ui-row
              tappable
              chevron
              heading={leaf.page.title}
              subheading={leaf.depth > 0 ? `In ${leaf.path.join(' › ')}` : undefined}
              icon="doc"
              icon-color={leaf.depth > 0 ? 'gray' : 'orange'}
              onpress={() => onopen(leaf.page)}
              data-testid="projects-page-{leaf.page.id}"
            ></ui-row>
          {/each}
        </ui-section>
      </ui-list>
    {:else}
      <ui-empty-state icon="doc" heading="No Pages" message="Name one above, and write it." data-testid="projects-pages-empty"></ui-empty-state>
    {/if}
  </div>
</section>

<style>
  .pages {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .new {
    padding: 8px 16px;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
</style>
