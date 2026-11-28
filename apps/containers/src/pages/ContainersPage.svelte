<!-- @page ContainersPage — the whole app: the sidebar beside what it shows (or one of the two in a narrow window): the containers of every stack or one, or Docker's images, volumes or networks; a container looked at in their place, the forms a container, a volume or a network is made in, the form an image recipe is kept and built in, the questions before one is removed, and what to do when Docker cannot be reached; driven by one window's Containers state -->
<script lang="ts">
  import ContainerForm from '../organisms/ContainerForm.svelte'
  import ContainerInspector from '../organisms/ContainerInspector.svelte'
  import ContainerList from '../organisms/ContainerList.svelte'
  import { EngineDown } from '@vos/docker'
  import ImageList from '../organisms/ImageList.svelte'
  import NetworkForm from '../organisms/NetworkForm.svelte'
  import NetworkList from '../organisms/NetworkList.svelte'
  import RecipeForm from '../organisms/RecipeForm.svelte'
  import RemoveContainer from '../organisms/RemoveContainer.svelte'
  import RemoveResource from '../organisms/RemoveResource.svelte'
  import StackSidebar from '../organisms/StackSidebar.svelte'
  import VolumeForm from '../organisms/VolumeForm.svelte'
  import VolumeList from '../organisms/VolumeList.svelte'
  import type { Containers } from '../lib/containers.svelte'
  import { isUp } from '../lib/format'
  import { statusOf } from '../lib/recipes.svelte'

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
  const resources = $derived(containers.resources)
  const recipes = $derived(containers.recipes)
  const scope = $derived(containers.scope)
  const reportOf = (kind: 'images' | 'volumes'): string | undefined => (resources.report?.kind === kind ? resources.report.text : undefined)
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
        counts={{ images: resources.images.length, volumes: resources.volumes.length, networks: resources.networks.length }}
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
      {:else if containers.form}
        <ContainerForm
          bind:form={containers.form}
          networks={containers.networks}
          volumes={containers.volumes}
          savable={containers.savable}
          onsave={() => void containers.save()}
          oncancel={() => containers.cancel()}
        />
      {:else if containers.removal}
        <RemoveContainer
          bind:removal={containers.removal}
          onremove={() => void containers.remove()}
          oncancel={() => (containers.removal = null)}
        />
      {:else if resources.volumeForm}
        <VolumeForm
          bind:form={resources.volumeForm}
          savable={resources.savable}
          onsave={() => void resources.save()}
          oncancel={() => resources.cancel()}
        />
      {:else if resources.networkForm}
        <NetworkForm
          bind:form={resources.networkForm}
          savable={resources.savable}
          onsave={() => void resources.save()}
          oncancel={() => resources.cancel()}
        />
      {:else if recipes.form}
        {@const recipe = recipes.editing}
        <RecipeForm
          bind:form={recipes.form}
          {recipe}
          status={statusOf(recipe, recipes.buildOf(recipe?.id ?? null), resources.images)}
          lines={recipes.buildOf(recipe?.id ?? null)?.lines ?? []}
          savable={recipes.savable}
          onsave={() => void recipes.save()}
          onbuild={() => void recipes.build()}
          oncancel={() => recipes.close()}
          onremove={() => void recipes.remove()}
        />
      {:else if resources.removal}
        <RemoveResource
          bind:removal={resources.removal}
          onremove={() => void resources.remove()}
          oncancel={() => (resources.removal = null)}
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
          onedit={() => void containers.open(inspected, 'edit')}
          onduplicate={() => void containers.open(inspected, 'duplicate')}
          onremove={() => containers.ask(inspected)}
        />
      {:else if scope.kind === 'images'}
        <ImageList
          images={resources.images}
          bind:pull={resources.pull}
          pruning={resources.pruning('images')}
          report={reportOf('images')}
          back={narrow}
          onback={() => containers.back()}
          onpull={() => void resources.pullImage()}
          onremove={(image) => resources.askImage(image)}
          onnew={(image) => void containers.compose(image.tags[0] ?? '')}
          onprune={() => void resources.prune('images')}
          recipes={recipes.all}
          statusOf={(recipe) => statusOf(recipe, recipes.buildOf(recipe.id), resources.images)}
          onrecipe={(recipe) => recipes.edit(recipe)}
          onnewrecipe={() => recipes.compose()}
        />
      {:else if scope.kind === 'volumes'}
        <VolumeList
          volumes={resources.volumes}
          pruning={resources.pruning('volumes')}
          report={reportOf('volumes')}
          back={narrow}
          onback={() => containers.back()}
          onremove={(volume) => resources.askVolume(volume)}
          onnew={() => resources.composeVolume()}
          onprune={() => void resources.prune('volumes')}
        />
      {:else if scope.kind === 'networks'}
        <NetworkList
          networks={resources.networks}
          back={narrow}
          onback={() => containers.back()}
          onremove={(network) => resources.askNetwork(network)}
          onnew={() => resources.composeNetwork()}
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
          onnew={() => void containers.compose()}
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
