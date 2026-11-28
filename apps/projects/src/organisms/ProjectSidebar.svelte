<!-- @organism ProjectSidebar — the projects, each with how many of its issues are still open, and the button that starts a new one; a tap on a project shows it -->
<script lang="ts">
  type Project = VosApp.Projects.Project

  interface Props {
    projects: readonly Project[]
    /** How many issues are still open in each project, by the project's id. */
    open: ReadonlyMap<number, number>
    /** The id of the project that is shown; none is marked when left out (a narrow window showing only the projects). */
    selected?: number
    onselect: (project: Project) => void
    onnew: () => void
  }

  let { projects, open, selected, onselect, onnew }: Props = $props()
</script>

<nav class="sidebar" aria-label="Projects">
  <header class="bar">
    <span class="grow"></span>
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="plain" square hit label="New Project" title="New Project" onclick={onnew} data-testid="projects-new">
      <ui-icon slot="icon" name="plus"></ui-icon>
    </ui-button>
  </header>
  <h1 class="ios-large-title">Projects</h1>
  <div class="scroll">
    <!-- an empty selectable list is not valid ARIA: the list is there once there is a project -->
    {#if projects.length > 0}
      <!-- a row's own name leaves its value out: each is named with its count, as it reads -->
      <ui-list variant="sidebar" selection="single" label="Projects" data-testid="projects-list">
        <ui-section>
          {#each projects as project (project.id)}
            <ui-row
              heading={project.name}
              subheading={project.key}
              value={String(open.get(project.id) ?? 0)}
              label="{project.name}, {project.key}, {open.get(project.id) ?? 0} open"
              icon="rectangle.stack"
              icon-color={project.color}
              selected={selected === project.id}
              onpress={() => onselect(project)}
              data-testid="projects-project-{project.key}"
            ></ui-row>
          {/each}
        </ui-section>
      </ui-list>
    {:else}
      <p class="none" data-testid="projects-list-empty">No Projects</p>
    {/if}
  </div>
</nav>

<style>
  .sidebar {
    box-sizing: border-box;
    min-width: 0;
    height: 100%;
    display: flex;
    flex-direction: column;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
    background: var(--bg-secondary);
  }
  .bar {
    display: flex;
    align-items: center;
    min-height: 44px;
    padding: 0 8px;
  }
  .grow {
    flex: 1;
  }
  h1 {
    margin: 0;
    padding: 0 16px 8px;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 8px;
  }
  .none {
    margin: 24px 8px;
    text-align: center;
    font: var(--type-subheadline);
    color: var(--label-secondary);
  }
</style>
