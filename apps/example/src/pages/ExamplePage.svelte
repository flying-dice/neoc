<!-- @page ExamplePage — the whole app: the sidebar of what there is to look at, and the part or the greeting picked, in two columns (one in a narrow window); driven by one window's Example state -->
<script lang="ts">
  import GreetingForm from '../organisms/GreetingForm.svelte'
  import PartDetail from '../organisms/PartDetail.svelte'
  import PartsSidebar from '../organisms/PartsSidebar.svelte'
  import type { Example } from '../lib/example.svelte'

  interface Props {
    /** This window's state; the composition root builds it with the real `api` and `host`, a story with a fake. */
    example: Example
  }

  let { example }: Props = $props()

  // in a window too narrow for both columns, the split view shows one: the kit says when, the same for every app
  let narrow = $state(false)
  const selected = $derived(example.selected)
  // something picked is in front; going back brings the sidebar
  const column = $derived(selected || example.saying ? 'detail' : 'sidebar')

  $effect(() => {
    void example.load()
    return example.listen()
  })
</script>

<main class="app" data-testid="example-app">
  {#if !example.loaded}
    <div class="waiting">
      {#if !example.failure}<ui-spinner size="large" label="Asking the backend" data-testid="example-loading"></ui-spinner>{/if}
    </div>
  {:else}
    <ui-split-view {column} oncollapsedchange={(e: CustomEvent<{ collapsed: boolean }>) => (narrow = e.detail.collapsed)}>
      <div slot="sidebar">
        <PartsSidebar parts={example.parts} selected={narrow ? undefined : (example.selectedId ?? undefined)} onselect={(id) => example.select(id)} />
      </div>
      <div class="pane">
        {#if example.saying}
          <GreetingForm
            name={example.name}
            greeting={example.greeting}
            refusal={example.refusal}
            ready={example.canGreet}
            sending={example.sending}
            back={narrow}
            ontype={(name) => example.type(name)}
            ongreet={() => void example.greet()}
            onback={() => example.select(null)}
          />
        {:else if selected}
          <PartDetail part={selected} back={narrow} onback={() => example.select(null)} />
        {:else}
          <div class="waiting">
            <ui-empty-state icon="star" heading="Nothing Selected" message="Choose a part of the app to read about it, or Say Hello to call the backend." data-testid="example-nothing"
            ></ui-empty-state>
          </div>
        {/if}
      </div>
    </ui-split-view>
  {/if}
  <!-- why the last call did not get through (the host's sentence), whatever is in front -->
  {#if example.failure}
    <ui-banner tone="error" data-testid="example-failure">{example.failure}</ui-banner>
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
  .pane {
    height: 100%;
    background: var(--bg-grouped);
  }
  .waiting {
    flex: 1;
    height: 100%;
    display: grid;
    place-items: center;
    padding: 16px;
  }
</style>
