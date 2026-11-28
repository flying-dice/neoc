<!-- @organism ProjectHeader — the project that is shown: its colour, name and key, the control that picks its board, its issues or its pages, and the way to delete it, after an alert asks -->
<script lang="ts">
  type Project = VosApp.Projects.Project
  import { TABS, type Tab } from '../lib/catalog'

  interface Props {
    project: Project
    tab: Tab
    /** Show the way back to the projects (a window too narrow for both). */
    back?: boolean
    ontab: (tab: Tab) => void
    onremove: (project: Project) => void
    onback?: () => void
  }

  let { project, tab, back = false, ontab, onremove, onback }: Props = $props()

  // the segmented control takes a list it may keep: a copy of the constant one
  const options = TABS.map((t) => ({ ...t }))
  // deleting a project deletes what is in it: the id of the project the person has been asked about
  let asking = $state<number | null>(null)
  const confirming = $derived(asking === project.id)
</script>

<header class="header">
  <div class="bar">
    <div class="lead">
    {#if back}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit label="Back to the projects" onclick={onback} data-testid="projects-back">
        <ui-icon slot="icon" name="chevron.left"></ui-icon>
        Projects
      </ui-button>
    {/if}
    </div>
    <div class="title">
      <span class="dot" style:background="var(--ios-{project.color})"></span>
      <h1 data-testid="projects-title">{project.name}</h1>
      <p class="key" data-testid="projects-key">{project.key}</p>
    </div>
    <div class="trail">
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button
      variant="plain"
      size="small"
      destructive
      hit
      disabled={confirming}
      onclick={() => (asking = project.id)}
      data-testid="projects-delete"
    >
      Delete Project
    </ui-button>
    </div>
  </div>
  <div class="tabs">
    <ui-segmented
      block
      hit
      label="What is shown of the project"
      {options}
      value={tab}
      onchange={(e) => ontab(e.detail.value as Tab)}
      data-testid="projects-tab"
    ></ui-segmented>
  </div>
  {#if confirming}
    <ui-alert
      open
      destructive
      action-label="Delete"
      heading="Delete {project.name}?"
      message="Its issues and its pages are deleted with it."
      data-testid="projects-confirm"
      oncancel={() => (asking = null)}
      onconfirm={() => {
        asking = null
        onremove(project)
      }}
    ></ui-alert>
  {/if}
</header>

<style>
  .header {
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
  }
  /* the title is centred in the bar, the way back before it and the delete after it */
  .bar {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
    align-items: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 12px 0 16px;
  }
  .lead {
    display: flex;
    justify-content: flex-start;
  }
  .trail {
    display: flex;
    justify-content: flex-end;
  }
  .title {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .dot {
    flex: none;
    width: 12px;
    height: 12px;
    border-radius: 50%;
  }
  h1 {
    margin: 0;
    min-width: 0;
    font: var(--type-headline);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .key {
    margin: 0;
    font: var(--type-subheadline);
    color: var(--label-secondary);
  }
  .tabs {
    max-width: 420px;
    margin: 0 auto;
    padding: 0 16px 4px;
  }
</style>
