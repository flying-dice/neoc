<!-- @molecule ProjectForm — a new project as it is typed, as a form of iPadOS is: a row each for its name and the key its issues are numbered under; what is wrong with them in the footer under the rows; and an Add button that cannot be pressed until there is something right to add -->
<script lang="ts">
  import type { Problems, ProjectDraft } from '../lib/projects.svelte'

  interface Props {
    /** What is typed so far. */
    draft: ProjectDraft
    /** What is wrong with each field as it is typed. */
    problems: Problems<'name' | 'key'>
    /** There is a name and a key, and nothing wrong with either. */
    ready: boolean
    /** The backend's word on the project that was sent and not added (another project has the key). */
    refused?: string | null
    /** A call is in flight: nothing more is added until it is answered. */
    busy?: boolean
    ondraft: (change: Partial<ProjectDraft>) => void
    onadd: () => void
    oncancel: () => void
  }

  let { draft, problems, ready, refused = null, busy = false, ondraft, onadd, oncancel }: Props = $props()

  const valueOf = (e: Event): string => (e.currentTarget as unknown as { value: string }).value

  // A field says what is wrong with it once the person has left it, not while they are still typing; from then on it
  // follows what is typed.
  let left = $state({ name: false, key: false })
  const nameProblem = $derived(left.name ? problems.name : null)
  const keyProblem = $derived(left.key ? problems.key : null)
  // one footer for the rows: the first thing that is wrong, top to bottom, then what the backend refused
  const problem = $derived(nameProblem ?? keyProblem ?? refused)
  const example = $derived(draft.key.trim() === '' ? 'WEB' : draft.key.trim())
</script>

<form
  class="project"
  novalidate
  onsubmit={(e) => {
    e.preventDefault()
    // Return in a field: what is wrong is said now
    left = { name: true, key: true }
    if (ready && !busy) onadd()
  }}
>
  <ui-list variant="insetGrouped" label="A new project">
    <ui-section
      footer={problem ?? `Its issues are numbered under the key: ${example}-1, ${example}-2.`}
      footer-tone={problem ? 'error' : 'note'}
      data-testid="projects-form"
    >
      <ui-row>
        <span class="field">Name</span>
        <ui-text-field
          variant="bare"
          label="Name"
          description={nameProblem ?? undefined}
          placeholder="Website"
          autocomplete="off"
          autocapitalize="words"
          enterkeyhint="next"
          maxlength={60}
          invalid={nameProblem !== null}
          value={draft.name}
          oninput={(e) => ondraft({ name: valueOf(e) })}
          onfocusout={() => (left.name = true)}
          data-testid="projects-form-name"
        ></ui-text-field>
      </ui-row>
      <ui-row>
        <span class="field">Key</span>
        <ui-text-field
          variant="bare"
          label="Key"
          description={(keyProblem ?? refused) || undefined}
          placeholder="WEB"
          autocomplete="off"
          autocapitalize="characters"
          enterkeyhint="done"
          maxlength={5}
          invalid={keyProblem !== null || (nameProblem === null && refused !== null)}
          value={draft.key}
          oninput={(e) => ondraft({ key: valueOf(e) })}
          onfocusout={() => (left.key = true)}
          data-testid="projects-form-key"
        ></ui-text-field>
      </ui-row>
    </ui-section>
  </ui-list>
  <div class="buttons">
    {#if busy}<ui-spinner label="Adding the project" data-testid="projects-form-busy"></ui-spinner>{/if}
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="gray" hit onclick={oncancel} data-testid="projects-form-cancel">Cancel</ui-button>
    <ui-button type="submit" hit disabled={!ready || busy} data-testid="projects-form-add">Add</ui-button>
  </div>
</form>

<style>
  .project {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  ui-section {
    margin-bottom: 0;
  }
  .field {
    flex: none;
    min-width: 64px;
  }
  .buttons {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    padding-inline: 16px;
  }
</style>
