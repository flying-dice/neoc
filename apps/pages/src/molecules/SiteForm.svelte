<!-- @molecule SiteForm — a site as it is typed, a new one or one being changed: what it is called, the folder its files are in, a port of its own, the hostnames it answers to, and the buttons that save it or give up -->
<script lang="ts">
  import type { SiteDraft } from '../lib/pages.svelte'

  interface Props {
    /** What is typed so far. */
    draft: SiteDraft
    /** A new site is typed, not one of the list. */
    adding?: boolean
    /** The port the sites with hostnames share, for the words under the field. */
    sharedPort: number
    /** A call is in flight: nothing is saved until it is answered. */
    busy?: boolean
    ondraft: (change: Partial<SiteDraft>) => void
    onsave: () => void
    oncancel: () => void
  }

  let { draft, adding = false, sharedPort, busy = false, ondraft, onsave, oncancel }: Props = $props()

  const valueOf = (e: Event): string => (e.currentTarget as unknown as { value: string }).value
  // a site with no name or folder, or no way to reach it, is not one yet
  const unfinished = $derived(
    draft.name.trim() === '' || draft.folder.trim() === '' || (draft.port.trim() === '' && draft.hostnames.trim() === '')
  )
</script>

<form
  class="site"
  aria-label={adding ? 'New site' : 'Change the site'}
  onsubmit={(e) => {
    e.preventDefault()
    if (!unfinished && !busy) onsave()
  }}
>
  <h2>{adding ? 'New Site' : 'Change the Site'}</h2>
  <label class="title" for="pages-site-name">Name</label>
  <ui-text-field
    id="pages-site-name"
    placeholder="Blog"
    autocomplete="off"
    enterkeyhint="next"
    maxlength={80}
    hit
    value={draft.name}
    oninput={(e) => ondraft({ name: valueOf(e) })}
    data-testid="pages-site-name"
  ></ui-text-field>
  <label class="title" for="pages-site-folder">Folder</label>
  <ui-text-field
    id="pages-site-folder"
    placeholder="/Users/me/Sites/blog"
    autocomplete="off"
    autocapitalize="off"
    spellcheck="false"
    enterkeyhint="next"
    maxlength={1024}
    hit
    value={draft.folder}
    oninput={(e) => ondraft({ folder: valueOf(e) })}
    data-testid="pages-site-folder"
  ></ui-text-field>
  <p class="hint">The full path of the folder the files are in. Its index.html is the front page, and nothing in it is changed.</p>
  <label class="title" for="pages-site-port">Port</label>
  <ui-text-field
    id="pages-site-port"
    placeholder="None"
    inputmode="numeric"
    autocomplete="off"
    enterkeyhint="next"
    maxlength={5}
    hit
    value={draft.port}
    oninput={(e) => ondraft({ port: valueOf(e) })}
    data-testid="pages-site-port"
  ></ui-text-field>
  <p class="hint">A port of its own, like 8081. Empty for a site with only hostnames.</p>
  <label class="title" for="pages-site-hostnames">Hostnames</label>
  <ui-text-field
    id="pages-site-hostnames"
    placeholder="blog.home"
    inputmode="url"
    autocomplete="off"
    autocapitalize="off"
    spellcheck="false"
    enterkeyhint="done"
    maxlength={600}
    hit
    value={draft.hostnames}
    oninput={(e) => ondraft({ hostnames: valueOf(e) })}
    data-testid="pages-site-hostnames"
  ></ui-text-field>
  <p class="hint">Names the site answers to on port {sharedPort}, apart by commas. Each must lead to this machine.</p>
  <div class="buttons">
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="plain" hit disabled={busy} onclick={oncancel} data-testid="pages-site-cancel">Cancel</ui-button>
    <ui-button type="submit" hit disabled={unfinished || busy} data-testid="pages-site-save">{adding ? 'Add Site' : 'Save'}</ui-button>
  </div>
</form>

<style>
  .site {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  h2 {
    margin: 0 0 4px;
    font: var(--type-headline);
  }
  .title {
    font: var(--type-subheadline);
    font-weight: 600;
  }
  /* a field with no words under it keeps the room they would take from the next label */
  ui-text-field {
    margin-bottom: 6px;
  }
  .hint {
    margin: -6px 0 6px;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
