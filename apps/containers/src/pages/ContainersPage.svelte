<!-- @page ContainersPage — the whole app: the stacks beside the containers of the one that is shown (or one of the two in a narrow window), a container looked at in their place, and what to do when Docker cannot be reached; driven by one window's Containers state -->
<script lang="ts">
  import ContainerInspector from '../organisms/ContainerInspector.svelte'
  import ContainerList from '../organisms/ContainerList.svelte'
  import { EngineDown } from '@vos/docker'
  import StackSidebar from '../organisms/StackSidebar.svelte'
  import type { Containers } from '../lib/containers.svelte'
  import { isUp } from '../lib/format'

  interface Props {
    /** This window's state; the composition root builds it with the real `rpc` and `events`, a story with a fake. */
    containers: Containers
    /** How often the logs that are shown are read again while their container runs, in milliseconds. */
    logsEvery?: number
  }

  let { containers, logsEvery = 2000 }: Props = $props()

  // in a window too narrow for both, the split view shows one column: the kit says when, the same for every app
  let narrow = $state(false)
  const engine = $derived(containers.engine)
  const inspected = $derived(containers.inspected)
  // a Docker that cannot be reached is said in the middle of the window, whichever column was in front
  const browsing = $derived(containers.browsing && (engine.reachable || !containers.loaded))

  $effect(() => {
    void containers.load()
    return containers.listen()
  })

  // Docker tells the app when a container changes, not when it writes: the logs of a running one are read again
  $effect(() => {
    if (!containers.following) return
    const timer = setInterval(() => void containers.refresh(), logsEvery)
    return () => clearInterval(timer)
  })
</script>

<main class="app" data-testid="containers-app">
  <ui-split-view column={browsing ? 'sidebar' : 'detail'} oncollapsedchange={(e) => (narrow = e.detail.collapsed)}>
    <div slot="sidebar">
      <StackSidebar
        {engine}
        total={containers.all.length}
        stacks={containers.stacks}
        selected={narrow ? undefined : containers.scope}
        draft={containers.hostDraft}
        onselect={(scope) => containers.select(scope)}
        ondraft={(address) => (containers.hostDraft = address)}
        onconnect={() => void containers.connect()}
      />
    </div>
    <div>
      {#if !containers.loaded}
        <div class="waiting">
          {#if !containers.failure}<ui-spinner size="large" label="Asking Docker" data-testid="containers-loading"></ui-spinner>{/if}
        </div>
      {:else if !engine.reachable}
        <EngineDown
          {engine}
          draft={containers.hostDraft}
          ondraft={(address) => (containers.hostDraft = address)}
          onconnect={() => void containers.connect()}
          testid="containers"
        />
      {:else if inspected}
        <ContainerInspector
          container={inspected}
          busy={containers.busy(inspected)}
          tab={containers.tab}
          logs={containers.logs}
          details={containers.details}
          onact={(action) => void containers.act(inspected, action)}
          ontab={(tab) => void containers.show(tab)}
          onback={() => containers.close()}
        />
      {:else}
        <ContainerList
          title={containers.title}
          containers={containers.containers}
          stack={containers.stack}
          stackBusy={containers.stack ? containers.stackBusy(containers.stack) : false}
          busy={(container) => containers.busy(container)}
          back={narrow}
          onopen={(container) => void containers.inspect(container)}
          ontoggle={(container) => void containers.act(container, isUp(container) ? 'stop' : 'start')}
          onstack={(stack, action) => void containers.actOnStack(stack, action)}
          onback={() => containers.back()}
        />
      {/if}
    </div>
  </ui-split-view>
  <!-- why the last call failed (Docker's own sentence, the backend's, or the host's), whichever column is in front -->
  {#if containers.failure}
    <ui-banner tone="error" data-testid="containers-failure">{containers.failure}</ui-banner>
  {/if}
</main>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg);
    color: var(--label);
    font: var(--type-body);
  }
  ui-split-view {
    flex: 1;
  }
  .waiting {
    height: 100%;
    display: grid;
    place-items: center;
  }
</style>
