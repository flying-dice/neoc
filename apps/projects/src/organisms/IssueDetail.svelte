<!-- @organism IssueDetail — one issue, open: its title to type over, the kind of work it is, its priority and its column as choices, what is known about it to write, and the way to delete it, after an alert asks -->
<script lang="ts">
  import type { Issue, IssueType, Priority, Status } from '@projects/rpc'
  import { PRIORITIES, STATUSES, TYPES } from '../lib/catalog'
  import { moment } from '../lib/dates'
  import type { IssueChanges } from '../lib/projects.svelte'

  interface Props {
    issue: Issue
    /** What the way back is called: where the issue was opened from. */
    from: string
    onchange: (issue: Issue, changes: IssueChanges) => void
    onremove: (issue: Issue) => void
    onback: () => void
  }

  let { issue, from, onchange, onremove, onback }: Props = $props()

  // the segmented controls take lists they may keep: copies of the constant ones
  const choices = (of: readonly { value: string; label: string }[]): { value: string; label: string }[] =>
    of.map(({ value, label }) => ({ value, label }))
  const types = choices(TYPES)
  const priorities = choices(PRIORITIES)
  const statuses = choices(STATUSES)

  let asking = $state<number | null>(null)
  const confirming = $derived(asking === issue.id)

  // What is typed in the title and the description is the window's until the backend has it: an answer to an earlier
  // change (a new status) draws the issue again, and must not put back the text that is being typed over.
  let typed = $state<{ issue: number; title: string | null; description: string | null }>({ issue: 0, title: null, description: null })
  const title = $derived(typed.issue === issue.id && typed.title !== null ? typed.title : issue.title)
  const description = $derived(typed.issue === issue.id && typed.description !== null ? typed.description : issue.description)

  function type(change: { title?: string; description?: string }): void {
    typed = { ...(typed.issue === issue.id ? typed : { title: null, description: null }), issue: issue.id, ...change }
  }
  $effect(() => {
    // the backend has what was typed: the issue speaks for itself again
    if (typed.issue !== issue.id) return
    if (typed.title !== null && typed.title.trim() === issue.title) typed.title = null
    if (typed.description !== null && typed.description === issue.description) typed.description = null
  })

  // The title commits when its field is left, and on Enter. It cannot be blank: the field takes back the one it has.
  function rename(): void {
    if (title.trim() === '') type({ title: issue.title })
    else onchange(issue, { title })
  }
</script>

<section class="issue" aria-label="Issue {issue.key}">
  <header class="bar">
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="plain" size="small" hit label="Back to {from}" onclick={onback} data-testid="projects-issue-back">
      <ui-icon slot="icon" name="chevron.left"></ui-icon>
      {from}
    </ui-button>
    <h1 data-testid="projects-issue-key">{issue.key}</h1>
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button
      variant="plain"
      square
      destructive
      hit
      label="Delete the issue"
      disabled={confirming}
      onclick={() => (asking = issue.id)}
      data-testid="projects-issue-remove"
    >
      <ui-icon slot="icon" name="trash"></ui-icon>
    </ui-button>
  </header>
  {#if confirming}
    <ui-alert
      open
      destructive
      action-label="Delete"
      heading="Delete {issue.key}?"
      message="It cannot be brought back."
      data-testid="projects-issue-confirm"
      oncancel={() => (asking = null)}
      onconfirm={() => {
        asking = null
        onremove(issue)
      }}
    ></ui-alert>
  {/if}
  <div class="scroll">
    <ui-list label="Issue {issue.key}">
      <ui-section>
        <ui-row>
          <form
            onsubmit={(e) => {
              e.preventDefault()
              rename()
            }}
          >
            <ui-text-field
              variant="bare"
              label="Title"
              autocomplete="off"
              enterkeyhint="done"
              maxlength={200}
              value={title}
              oninput={(e) => type({ title: (e.currentTarget as unknown as { value: string }).value })}
              onchange={rename}
              data-testid="projects-issue-title"
            ></ui-text-field>
          </form>
        </ui-row>
      </ui-section>
      <ui-section>
        <ui-row heading="Status">
          <ui-segmented
            slot="trailing"
            label="Status"
            options={statuses}
            value={issue.status}
            onchange={(e) => onchange(issue, { status: e.detail.value as Status })}
            data-testid="projects-issue-status"
          ></ui-segmented>
        </ui-row>
        <ui-row heading="Type">
          <ui-segmented
            slot="trailing"
            label="Type"
            options={types}
            value={issue.type}
            onchange={(e) => onchange(issue, { type: e.detail.value as IssueType })}
            data-testid="projects-issue-type"
          ></ui-segmented>
        </ui-row>
        <ui-row heading="Priority">
          <ui-segmented
            slot="trailing"
            label="Priority"
            options={priorities}
            value={issue.priority}
            onchange={(e) => onchange(issue, { priority: e.detail.value as Priority })}
            data-testid="projects-issue-priority"
          ></ui-segmented>
        </ui-row>
      </ui-section>
    </ui-list>
    <div class="description">
      <h2>Description</h2>
      <ui-text-area
        label="Description"
        placeholder="What is known about it"
        maxlength={5000}
        rows={6}
        value={description}
        oninput={(e) => type({ description: (e.currentTarget as unknown as { value: string }).value })}
        onchange={() => onchange(issue, { description })}
        data-testid="projects-issue-description"
      ></ui-text-area>
      <p class="edited" data-testid="projects-issue-edited">Edited {moment(issue.updated)}</p>
    </div>
  </div>
</section>

<style>
  .issue {
    box-sizing: border-box;
    height: 100%;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
    display: flex;
    flex-direction: column;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 8px 0 12px;
  }
  h1 {
    flex: 1;
    margin: 0;
    text-align: center;
    font: var(--type-headline);
    font-variant-numeric: tabular-nums;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  form {
    flex: 1;
    min-width: 0;
  }
  ui-text-field {
    display: block;
  }
  .description {
    padding: 0 16px 20px;
  }
  h2 {
    margin: 0 0 6px;
    padding-inline: 16px;
    font: var(--type-footnote);
    color: var(--label-secondary);
    text-transform: uppercase;
  }
  .edited {
    margin: 6px 0 0;
    padding-inline: 16px;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
</style>
