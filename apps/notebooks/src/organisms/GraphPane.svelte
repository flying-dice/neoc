<!-- @organism GraphPane — the graph of a notebook: a dot for each note, a line for each link between two, laid out so that notes that link to each other are near each other; a dot opens its note -->
<script lang="ts">
  import { layout, type GraphLink, type GraphNode } from '../lib/graph'

  interface Props {
    /** The name of the notebook. */
    title: string
    nodes: readonly GraphNode[]
    links: readonly GraphLink[]
    /** The id of the note that is open: its dot is marked. */
    selected?: number
    /** Show the way back to the list (a window too narrow for both). */
    back?: boolean
    onopen: (id: number) => void
    onback?: () => void
  }

  let { title, nodes, links, selected, back = false, onopen, onback }: Props = $props()

  const LABEL_MAX = 22

  let width = $state(600)
  let height = $state(400)
  const graph = $derived(layout(nodes, links, Math.max(width, 120), Math.max(height, 120), 48))

  // A long title is cut after a whole word, with no mark of its own: what is shown stays part of the note's name.
  function short(name: string): string {
    if ([...name].length <= LABEL_MAX) return name
    const cut = [...name].slice(0, LABEL_MAX).join('')
    return cut.includes(' ') ? cut.slice(0, cut.lastIndexOf(' ')) : cut
  }
  const radiusOf = (degree: number): number => Math.min(6 + degree * 1.5, 14)

  function keyed(event: KeyboardEvent, id: number): void {
    if (event.key !== 'Enter' && event.key !== ' ') return
    event.preventDefault()
    onopen(id)
  }
</script>

<section class="pane" aria-label="Graph of {title}">
  <header class="bar">
    {#if back}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit label="Back to the notes" onclick={onback} data-testid="notebooks-back">
        <ui-icon slot="icon" name="chevron.left"></ui-icon>
        Notes
      </ui-button>
    {/if}
    <h1 data-testid="notebooks-graph-title">Graph</h1>
    <p class="count" data-testid="notebooks-graph-count">
      {nodes.length}
      {nodes.length === 1 ? 'note' : 'notes'}, {graph.edges.length}
      {graph.edges.length === 1 ? 'link' : 'links'}
    </p>
  </header>
  <div class="canvas" bind:clientWidth={width} bind:clientHeight={height}>
    {#if nodes.length === 0}
      <ui-empty-state icon="network" heading="No Notes" message="A note is a dot here, and a link a line." data-testid="notebooks-graph-empty"></ui-empty-state>
    {:else}
      <svg viewBox="0 0 {Math.max(width, 120)} {Math.max(height, 120)}" role="group" aria-label="Notes and their links" data-testid="notebooks-graph">
        {#each graph.edges as edge (`${edge.from.id}-${edge.to.id}`)}
          <line x1={edge.from.x} y1={edge.from.y} x2={edge.to.x} y2={edge.to.y} class:near={edge.from.id === selected || edge.to.id === selected} />
        {/each}
        {#each graph.nodes as node (node.id)}
          <g
            class="node"
            class:selected={node.id === selected}
            role="button"
            tabindex="0"
            aria-label={node.title}
            onclick={() => onopen(node.id)}
            onkeydown={(e) => keyed(e, node.id)}
            data-testid="notebooks-node-{node.id}"
          >
            <!-- a dot is small: the ring around it is what is pressed -->
            <circle class="hit" cx={node.x} cy={node.y} r="22" />
            <circle class="dot" cx={node.x} cy={node.y} r={radiusOf(node.degree)} />
            <text x={node.x} y={node.y + radiusOf(node.degree) + 14} text-anchor="middle">{short(node.title)}</text>
          </g>
        {/each}
      </svg>
    {/if}
  </div>
</section>

<style>
  .pane {
    box-sizing: border-box;
    min-width: 0;
    height: 100%;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
    display: flex;
    flex-direction: column;
  }
  .bar {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-height: 44px;
    padding: 0 16px 6px;
  }
  h1 {
    margin: 0;
    font: var(--type-title2);
    font-weight: 700;
  }
  .count {
    margin: 0;
    font: var(--type-subheadline);
    color: var(--label-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .canvas {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
  svg {
    display: block;
    width: 100%;
    height: 100%;
  }
  line {
    stroke: var(--separator-opaque);
    stroke-width: 1.5;
  }
  line.near {
    stroke: var(--tint);
  }
  .node {
    cursor: pointer;
    outline: none;
  }
  .hit {
    fill: transparent;
  }
  .dot {
    fill: var(--ios-gray);
  }
  .node:hover .dot,
  .selected .dot {
    fill: var(--tint);
  }
  .node:focus-visible .hit {
    stroke: var(--focus-ring);
    stroke-width: 2;
  }
  text {
    fill: var(--label-secondary);
    font: var(--type-caption1);
    pointer-events: none;
  }
  .selected text {
    fill: var(--label);
    font-weight: 600;
  }
</style>
