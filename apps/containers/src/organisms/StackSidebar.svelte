<!-- @organism StackSidebar — what there is to show: every container, Docker's images, volumes and networks with how many there are, or one Compose stack with how much of it runs; and under them which Docker this is, with the field that chooses another -->
<script lang="ts">
  type Engine = VosApp.Containers.Engine
  type Stack = VosApp.Containers.Stack
  import { ConnectionForm, type Tone } from '@vos/docker'
  import { ALL, IMAGES, NETWORKS, VOLUMES, sameScope, stackScope, type Scope } from '../lib/containers.svelte'
  import { stackSummary, stackTone } from '../lib/format'

  interface Props {
    engine: Engine
    /** How many containers there are in all. */
    total: number
    /** How many images, volumes and networks Docker has. */
    counts: { images: number; volumes: number; networks: number }
    stacks: readonly Stack[]
    /** What is shown; nothing is marked when left out (a narrow window showing only this). */
    selected?: Scope
    /** The address typed for another Docker. */
    draft: string
    onselect: (scope: Scope) => void
    ondraft: (address: string) => void
    onconnect: () => void
  }

  let { engine, total, counts, stacks, selected, draft, onselect, ondraft, onconnect }: Props = $props()

  // what Docker has beside its containers, each a list of its own
  const OBJECTS = [
    { scope: IMAGES, kind: 'images', heading: 'Images', icon: 'doc', color: 'indigo' },
    { scope: VOLUMES, kind: 'volumes', heading: 'Volumes', icon: 'folder', color: 'orange' },
    { scope: NETWORKS, kind: 'networks', heading: 'Networks', icon: 'network', color: 'blue' }
  ] as const

  const isSelected = (scope: Scope): boolean => selected !== undefined && sameScope(selected, scope)
  const COLORS: Readonly<Record<Tone, string>> = { on: 'green', busy: 'orange', off: 'gray', bad: 'red' }

  // the field that chooses another Docker is out of the way until it is asked for
  let choosing = $state(false)
  const where = $derived(engine.host === '' ? 'This machine' : engine.host)
</script>

<nav class="sidebar" aria-label="Docker">
  <div class="lists">
    <!-- a row's own name leaves its value out: it is named with its count, as it reads -->
    <ui-list variant="sidebar" selection="single" label="Docker" data-testid="containers-scopes">
      <ui-section>
        <ui-row
          heading="All Containers"
          value={String(total)}
          label="All Containers, {total}"
          icon="rectangle.stack"
          icon-color="teal"
          selected={isSelected(ALL)}
          onpress={() => onselect(ALL)}
          data-testid="containers-scope-all"
        ></ui-row>
        {#each OBJECTS as object (object.kind)}
          <ui-row
            heading={object.heading}
            value={String(counts[object.kind])}
            label="{object.heading}, {counts[object.kind]}"
            icon={object.icon}
            icon-color={object.color}
            selected={isSelected(object.scope)}
            onpress={() => onselect(object.scope)}
            data-testid="containers-scope-{object.kind}"
          ></ui-row>
        {/each}
      </ui-section>
    </ui-list>
    <!-- an empty selectable list is not valid ARIA: the list is there once there is a stack -->
    {#if stacks.length > 0}
      <ui-list variant="sidebar" selection="single" label="Compose stacks" data-testid="containers-stacks">
        <ui-section header="Compose Stacks">
          {#each stacks as stack (stack.name)}
            <ui-row
              heading={stack.name}
              subheading={stackSummary(stack)}
              icon="square.grid"
              icon-color={COLORS[stackTone(stack)]}
              selected={isSelected(stackScope(stack.name))}
              onpress={() => onselect(stackScope(stack.name))}
              data-testid="containers-stack-{stack.name}"
            ></ui-row>
          {/each}
        </ui-section>
      </ui-list>
    {:else if engine.reachable}
      <p class="none" data-testid="containers-no-stacks">No Compose stacks. One appears here when it is brought up with <code>docker compose up</code>.</p>
    {/if}
  </div>
  <div class="engine">
    <div class="which">
      <p class="about" data-testid="containers-engine">
        <span class="name">{engine.reachable ? `Docker ${engine.version}` : 'Docker'}</span>
        <span class="where">{where}</span>
      </p>
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button
        variant={choosing ? 'tinted' : 'plain'}
        square
        hit
        toggle
        pressed={choosing}
        label="Choose another Docker"
        onclick={() => (choosing = !choosing)}
        data-testid="containers-choose"
      >
        <ui-icon slot="icon" name="gear"></ui-icon>
      </ui-button>
    </div>
    <!-- while Docker cannot be reached the same field is in the middle of the window, with why -->
    {#if choosing && engine.reachable}
      <ConnectionForm {draft} host={engine.host} {ondraft} {onconnect} testid="containers" />
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
  .lists {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 8px;
  }
  .none {
    margin: 16px 12px;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  code {
    font-family: var(--font-mono);
  }
  .engine {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px 16px 12px;
    border-top: 1px solid var(--separator);
  }
  .which {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .about {
    flex: 1;
    min-width: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
  }
  .name {
    font: var(--type-subheadline);
    font-weight: 600;
  }
  .where {
    font: var(--type-footnote);
    color: var(--label-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
