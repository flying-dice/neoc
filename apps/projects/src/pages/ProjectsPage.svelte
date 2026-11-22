<!-- @page ProjectsPage — the whole app: the projects beside the one that is shown (its board, its issues or its pages, or the issue or the page that is open), or one of the two in a narrow window; driven by one window's Projects state -->
<script lang="ts">
  import ProjectForm from '../molecules/ProjectForm.svelte'
  import Board from '../organisms/Board.svelte'
  import IssueDetail from '../organisms/IssueDetail.svelte'
  import IssueList from '../organisms/IssueList.svelte'
  import PageList from '../organisms/PageList.svelte'
  import PageView from '../organisms/PageView.svelte'
  import ProjectHeader from '../organisms/ProjectHeader.svelte'
  import ProjectSidebar from '../organisms/ProjectSidebar.svelte'
  import type { Projects } from '../lib/projects.svelte'

  interface Props {
    /** This window's state; the composition root builds it with the real `api` and `host`, a story with a fake. */
    projects: Projects
  }

  let { projects }: Props = $props()

  // below 720 wide the projects and the one that is shown are one column each: `browsing` decides which one shows
  let width = $state(1040)
  const narrow = $derived(width < 720)

  $effect(() => {
    void projects.load()
    return projects.listen()
  })
</script>

<main class="projects" bind:clientWidth={width} data-testid="projects-app">
  <div class="columns" class:narrow class:browsing={projects.browsing}>
    <div class="side">
      <ProjectSidebar
        projects={projects.projects}
        open={projects.open}
        selected={narrow || projects.form ? undefined : projects.project?.id}
        onselect={(project) => projects.select(project)}
        onnew={() => projects.begin()}
      />
    </div>
    <div class="detail">
      {#if projects.form}
        <section class="starting" aria-label="New Project" data-testid="projects-starting">
          <h1>New Project</h1>
          <ProjectForm
            draft={projects.form}
            problems={projects.problems}
            ready={projects.ready}
            refused={projects.refused}
            busy={projects.busy}
            ondraft={(change) => projects.draft(change)}
            onadd={() => void projects.addProject()}
            oncancel={() => projects.cancel()}
          />
        </section>
      {:else if projects.issue}
        <IssueDetail
          issue={projects.issue}
          from={projects.tab === 'issues' ? 'Issues' : 'Board'}
          onchange={(issue, changes) => void projects.changeIssue(issue, changes)}
          onremove={(issue) => void projects.removeIssue(issue)}
          onback={() => projects.closeIssue()}
        />
      {:else if projects.page}
        <PageView
          leaf={projects.page}
          body={projects.body}
          edit={projects.edit}
          subpages={projects.subpages}
          draft={projects.pageDraft}
          onrewrite={() => projects.rewrite()}
          ontype={(change) => projects.type(change)}
          onsave={() => void projects.save()}
          ondiscard={() => projects.discard()}
          onremove={(page) => void projects.removePage(page)}
          onopen={(page) => void projects.openPage(page)}
          ondraft={(text) => (projects.pageDraft = text)}
          onadd={() => void projects.addPage()}
          onback={() => void projects.closePage()}
        />
      {:else if projects.project}
        <div class="project">
          <ProjectHeader
            project={projects.project}
            tab={projects.tab}
            back={narrow}
            ontab={(tab) => projects.show(tab)}
            onremove={(project) => void projects.removeProject(project)}
            onback={() => projects.back()}
          />
          <div class="part">
            {#if projects.tab === 'board'}
              <Board
                columns={projects.columns}
                draft={projects.issueDraft}
                onopen={(issue) => projects.openIssue(issue)}
                onmove={(issue, status) => void projects.move(issue, status)}
                ondraft={(text) => (projects.issueDraft = text)}
                onadd={() => void projects.addIssue()}
              />
            {:else if projects.tab === 'issues'}
              <IssueList
                issues={projects.issues}
                draft={projects.issueDraft}
                onopen={(issue) => projects.openIssue(issue)}
                ondraft={(text) => (projects.issueDraft = text)}
                onadd={() => void projects.addIssue()}
              />
            {:else}
              <PageList
                pages={projects.pages}
                draft={projects.pageDraft}
                onopen={(page) => void projects.openPage(page)}
                ondraft={(text) => (projects.pageDraft = text)}
                onadd={() => void projects.addPage()}
              />
            {/if}
          </div>
        </div>
      {:else if projects.loaded}
        <div class="none">
          <ui-empty-state
            icon="rectangle.stack"
            heading="No Projects"
            message="A project keeps issues on a board and pages of writing beside them."
            data-testid="projects-empty"
          >
            <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <ui-button slot="action" hit onclick={() => projects.begin()} data-testid="projects-empty-new">New Project</ui-button>
          </ui-empty-state>
        </div>
      {/if}
    </div>
  </div>
  <!-- why the last call failed (the backend's own sentence, or the host's), whichever column is in front -->
  {#if projects.failure}
    <ui-banner tone="error" data-testid="projects-failure">{projects.failure}</ui-banner>
  {/if}
</main>

<style>
  .projects {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg-grouped);
    color: var(--label);
    font: var(--type-body);
  }
  .columns {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(220px, 280px) minmax(0, 1fr);
  }
  .side {
    min-width: 0;
    min-height: 0;
    border-inline-end: 1px solid var(--separator);
  }
  .detail {
    min-width: 0;
    min-height: 0;
  }
  .project {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .part {
    flex: 1;
    min-height: 0;
  }
  .starting {
    box-sizing: border-box;
    max-width: 560px;
    margin: 0 auto;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
  }
  .starting h1 {
    margin: 0;
    padding: 8px 32px 4px;
    font: var(--type-title2);
    font-weight: 700;
  }
  .none {
    height: 100%;
    display: grid;
    place-items: center;
  }
  /* one column: the projects, or the one that is picked */
  .narrow {
    grid-template-columns: minmax(0, 1fr);
  }
  .narrow:not(.browsing) .side,
  .narrow.browsing .detail {
    display: none;
  }
  .narrow .side {
    border-inline-end: 0;
  }
</style>
