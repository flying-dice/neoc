<!-- @organism PartsSidebar — what there is to look at: the parts of a vos app the backend serves, and under them the row that opens the greeting -->
<script lang="ts">
  import type { Part } from 'app:rpc'
  import { HELLO } from '../lib/example.svelte'

  interface Props {
    parts: readonly Part[]
    /** The id of the row picked; nothing is marked when left out (a narrow window showing only this). */
    selected?: string
    onselect: (id: string) => void
  }

  let { parts, selected, onselect }: Props = $props()
</script>

<nav class="sidebar" aria-label="Example">
  <h1>Example</h1>
  <div class="lists">
    <ui-list variant="sidebar" selection="single" label="Example" data-testid="example-sidebar">
      <ui-section header="Parts of an App">
        {#each parts as part (part.id)}
          <ui-row heading={part.title} icon={part.icon} icon-color="blue" selected={selected === part.id} onpress={() => onselect(part.id)} data-testid="example-part-{part.id}"
          ></ui-row>
        {/each}
      </ui-section>
      <ui-section header="Try It">
        <ui-row heading="Say Hello" icon="message" icon-color="green" selected={selected === HELLO} onpress={() => onselect(HELLO)} data-testid="example-hello"></ui-row>
      </ui-section>
    </ui-list>
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
  h1 {
    margin: 0;
    padding: 0 16px 8px;
    font: var(--type-title2);
    font-weight: 700;
  }
  .lists {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 8px;
  }
</style>
