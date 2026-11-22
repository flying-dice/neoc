<!-- @organism IssueList — every issue of a project in the order they were added, each with its key, the kind of work it is, its priority and the column it is in, and the field the next one is typed in; a tap opens an issue -->
<script lang="ts">
  import type { Issue } from 'app:rpc'
  import Composer from '../molecules/Composer.svelte'
  import { priorityOf, statusOf, typeOf } from '../lib/catalog'

  interface Props {
    issues: readonly Issue[]
    /** What is typed as the next issue. */
    draft: string
    onopen: (issue: Issue) => void
    ondraft: (text: string) => void
    onadd: () => void
  }

  let { issues, draft, onopen, ondraft, onadd }: Props = $props()
</script>

<section class="issues" aria-label="Issues">
  <div class="new">
    <Composer {draft} label="New Issue" action="Add the issue" testid="projects-new-issue" {ondraft} {onadd} />
  </div>
  <div class="scroll">
    {#if issues.length > 0}
      <ui-list label="Issues" data-testid="projects-issues">
        <ui-section>
          {#each issues as issue (issue.id)}
            {@const type = typeOf(issue.type)}
            {@const about = `${issue.key} · ${type.label} · ${priorityOf(issue.priority).label}`}
            <ui-row
              tappable
              chevron
              heading={issue.title}
              subheading={about}
              value={statusOf(issue.status).label}
              label="{issue.title}, {about}, {statusOf(issue.status).label}"
              icon={type.icon}
              icon-color={type.color}
              onpress={() => onopen(issue)}
              data-testid="projects-issue-{issue.id}"
            ></ui-row>
          {/each}
        </ui-section>
      </ui-list>
    {:else}
      <ui-empty-state icon="checkmark" heading="No Issues" message="Type one above." data-testid="projects-issues-empty"></ui-empty-state>
    {/if}
  </div>
</section>

<style>
  .issues {
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
