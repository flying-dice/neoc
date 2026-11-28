<!-- @organism Board — a project's issues in the three columns of its board, the most pressing first, with the field the next one is typed in; an issue is moved by dragging its card to another column, or with the button on the card -->
<script lang="ts">
  type Issue = VosApp.Projects.Issue
  type Status = VosApp.Projects.Status
  import Composer from '../molecules/Composer.svelte'
  import IssueCard from '../molecules/IssueCard.svelte'
  import { STATUSES } from '../lib/catalog'

  interface Props {
    /** The issues of each column, in the order they are shown. */
    columns: Readonly<Record<Status, readonly Issue[]>>
    /** What is typed as the next issue. */
    draft: string
    onopen: (issue: Issue) => void
    onmove: (issue: Issue, status: Status) => void
    ondraft: (text: string) => void
    onadd: () => void
  }

  let { columns, draft, onopen, onmove, ondraft, onadd }: Props = $props()

  // the card that is being dragged, and the column it is over
  let dragged = $state<Issue | null>(null)
  let over = $state<Status | null>(null)

  function drop(status: Status): void {
    const issue = dragged
    dragged = null
    over = null
    if (issue && issue.status !== status) onmove(issue, status)
  }
</script>

<section class="board" aria-label="Board" data-testid="projects-board">
  <div class="new">
    <Composer {draft} label="New Issue" action="Add the issue" testid="projects-new-issue" {ondraft} {onadd} />
  </div>
  <div class="columns">
    {#each STATUSES as status, at (status.value)}
      {@const issues = columns[status.value]}
      <!-- a column takes a dragged card; the same move is the button on the card, for a keyboard and a finger -->
      <section
        class="column"
        class:over={over === status.value && dragged?.status !== status.value}
        aria-label={status.label}
        ondragover={(e) => {
          if (!dragged) return
          e.preventDefault()
          over = status.value
        }}
        ondragleave={() => {
          if (over === status.value) over = null
        }}
        ondrop={(e) => {
          e.preventDefault()
          drop(status.value)
        }}
        data-testid="projects-column-{status.value}"
      >
        <header>
          <span class="dot" style:background="var(--ios-{status.color})"></span>
          <h2>{status.label}</h2>
          <p class="count" data-testid="projects-column-{status.value}-count">{issues.length}</p>
        </header>
        {#if issues.length > 0}
          <ul class="cards" data-testid="projects-column-{status.value}-cards">
            {#each issues as issue (issue.id)}
              <IssueCard
                {issue}
                next={STATUSES[at + 1]?.label}
                onopen={() => onopen(issue)}
                onadvance={() => {
                  const next = STATUSES[at + 1]
                  if (next) onmove(issue, next.value)
                }}
                ondrag={(dragging) => {
                  dragged = dragging ? issue : null
                  if (!dragging) over = null
                }}
              />
            {/each}
          </ul>
        {:else}
          <p class="none" data-testid="projects-column-{status.value}-empty">No Issues</p>
        {/if}
      </section>
    {/each}
  </div>
</section>

<style>
  .board {
    container-type: inline-size;
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .new {
    padding: 8px 16px;
  }
  .columns {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    align-items: start;
    gap: 12px;
    padding: 4px 16px 16px;
  }
  .column {
    min-width: 0;
    padding: 8px;
    border-radius: var(--radius-lg);
    background: var(--fill-quaternary);
  }
  .column.over {
    outline: 2px solid var(--tint);
    outline-offset: -2px;
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 8px 8px;
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
  h2 {
    margin: 0;
    font: var(--type-subheadline);
    font-weight: 600;
  }
  .count {
    margin: 0;
    font: var(--type-subheadline);
    color: var(--label-secondary);
    font-variant-numeric: tabular-nums;
  }
  .cards {
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .none {
    margin: 0;
    padding: 12px 8px 16px;
    text-align: center;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  /* too narrow for three columns side by side: one under the other */
  @container (max-width: 560px) {
    .columns {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
