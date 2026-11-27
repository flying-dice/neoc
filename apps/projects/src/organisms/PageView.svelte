<!-- @organism PageView — one page, open: its title, when it last changed and its text to read, with the pages under it and the field the next of those is named in; Edit turns the title and the text into fields, and Done keeps them; it is deleted, with the pages under it, after an alert asks -->
<script lang="ts">
  type Page = VosApp.Projects.Page
  import Composer from '../molecules/Composer.svelte'
  import type { Leaf } from '../lib/catalog'
  import { moment } from '../lib/dates'
  import type { PageDraft } from '../lib/projects.svelte'

  interface Props {
    leaf: Leaf
    /** The text of the page; null until it has been read. */
    body: string | null
    /** The page as it is being rewritten, while it is. */
    edit: PageDraft | null
    /** The pages directly under it. */
    subpages: readonly Page[]
    /** What is typed as the title of the next page under it. */
    draft: string
    onrewrite: () => void
    ontype: (change: Partial<PageDraft>) => void
    onsave: () => void
    ondiscard: () => void
    onremove: (page: Page) => void
    onopen: (page: Page) => void
    ondraft: (text: string) => void
    onadd: () => void
    onback: () => void
  }

  let { leaf, body, edit, subpages, draft, onrewrite, ontype, onsave, ondiscard, onremove, onopen, ondraft, onadd, onback }: Props = $props()

  const page = $derived(leaf.page)
  // the way back is to the page it is under, or to the list of pages
  const from = $derived(leaf.path.at(-1) ?? 'Pages')

  let asking = $state<number | null>(null)
  const confirming = $derived(asking === page.id)
</script>

<section class="page" aria-label="Page {page.title}">
  <header class="bar">
    {#if edit}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit onclick={ondiscard} data-testid="projects-page-cancel">Cancel</ui-button>
      <span class="gap"></span>
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button size="small" hit disabled={edit.title.trim() === ''} onclick={onsave} data-testid="projects-page-done">Done</ui-button>
    {:else}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit label="Back to {from}" onclick={onback} data-testid="projects-page-back">
        <ui-icon slot="icon" name="chevron.left"></ui-icon>
        <span class="from">{from}</span>
      </ui-button>
      <span class="gap"></span>
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit disabled={body === null} onclick={onrewrite} data-testid="projects-page-edit">Edit</ui-button>
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button
        variant="plain"
        square
        destructive
        hit
        label="Delete the page"
        disabled={confirming}
        onclick={() => (asking = page.id)}
        data-testid="projects-page-remove"
      >
        <ui-icon slot="icon" name="trash"></ui-icon>
      </ui-button>
    {/if}
  </header>
  {#if confirming}
    <ui-alert
      open
      destructive
      action-label="Delete"
      heading="Delete {page.title}?"
      message={subpages.length > 0 ? 'The pages under it are deleted with it.' : 'It cannot be brought back.'}
      data-testid="projects-page-confirm"
      oncancel={() => (asking = null)}
      onconfirm={() => {
        asking = null
        onremove(page)
      }}
    ></ui-alert>
  {/if}
  {#if edit}
    <div class="writing">
      <ui-text-field
        label="Title"
        placeholder="Title"
        autocomplete="off"
        enterkeyhint="next"
        maxlength={120}
        value={edit.title}
        oninput={(e) => ontype({ title: (e.currentTarget as unknown as { value: string }).value })}
        data-testid="projects-page-title-field"
      ></ui-text-field>
      <ui-text-area
        label="Text"
        placeholder="Start writing"
        maxlength={20000}
        value={edit.body}
        oninput={(e) => ontype({ body: (e.currentTarget as unknown as { value: string }).value })}
        data-testid="projects-page-text"
      ></ui-text-area>
    </div>
  {:else}
    <div class="scroll">
      <article>
        {#if leaf.depth > 0}<p class="path" data-testid="projects-page-path">{leaf.path.join(' › ')}</p>{/if}
        <h1 data-testid="projects-page-title">{page.title}</h1>
        <p class="edited" data-testid="projects-page-edited">Edited {moment(page.updated)}</p>
        {#if body === null}
          <ui-spinner label="Reading the page" data-testid="projects-page-reading"></ui-spinner>
        {:else if body.trim() === ''}
          <p class="blank" data-testid="projects-page-blank">Nothing is written here yet.</p>
        {:else}
          <p class="body" data-testid="projects-page-body">{body}</p>
        {/if}
      </article>
      {#if subpages.length > 0}
        <ui-list label="Pages under {page.title}" data-testid="projects-subpages">
          <ui-section header="Pages Under It">
            {#each subpages as sub (sub.id)}
              <ui-row
                tappable
                chevron
                heading={sub.title}
                icon="doc"
                icon-color="gray"
                onpress={() => onopen(sub)}
                data-testid="projects-subpage-{sub.id}"
              ></ui-row>
            {/each}
          </ui-section>
        </ui-list>
      {/if}
      <div class="new">
        <Composer {draft} label="New Page Under It" action="Add the page" testid="projects-new-subpage" {ondraft} {onadd} />
      </div>
    </div>
  {/if}
</section>

<style>
  .page {
    box-sizing: border-box;
    height: 100%;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
    display: flex;
    flex-direction: column;
    background: var(--bg);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 12px;
  }
  .gap {
    flex: 1;
  }
  .from {
    display: inline-block;
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    vertical-align: bottom;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  /* a column of text a line of which can be read in one go */
  article,
  .writing {
    box-sizing: border-box;
    width: 100%;
    max-width: 720px;
    margin: 0 auto;
    padding: 8px 24px 16px;
  }
  .path {
    margin: 0 0 2px;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  h1 {
    margin: 0;
    font: var(--type-large-title);
    font-weight: 700;
    overflow-wrap: anywhere;
  }
  .edited {
    margin: 2px 0 16px;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  .body {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .blank {
    margin: 0;
    color: var(--label-secondary);
  }
  ui-list,
  .new {
    box-sizing: border-box;
    display: block;
    max-width: 720px;
    margin: 0 auto;
  }
  .new {
    padding: 0 24px 20px;
  }
  .writing {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  /* the text takes the room the title leaves */
  ui-text-area {
    flex: 1;
  }
</style>
