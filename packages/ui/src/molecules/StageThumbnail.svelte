<!--
  @molecule StageThumbnail — a recent-apps strip entry: miniature window
  cards for each app in the group, with app icons overlapping the corner.
  Draggable onto the stage to merge groups. Atoms: IconTile, Icon.
  Test id: `testid` on the thumbnail button.
-->
<script lang="ts">
  import IconTile from '../atoms/IconTile.svelte'
  import Icon from '../atoms/Icon.svelte'
  import type { StageSummary } from '../types'

  interface Props {
    testid: string
    stage: StageSummary
    onclick?: () => void
  }

  let { testid, stage, onclick }: Props = $props()

  const label = $derived(stage.apps.map((a) => a.name).join(' + '))

  function ondragstart(e: DragEvent): void {
    e.dataTransfer?.setData('application/x-vos-stage', stage.id)
    if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move'
  }
</script>

<button
  type="button"
  class="thumb"
  data-testid={testid}
  draggable="true"
  aria-label="Show {label}"
  title={label}
  {onclick}
  {ondragstart}
>
  <span class="cards">
    {#each stage.apps.slice(0, 3) as app, i (app.id)}
      <span class="card" style:--i={i} style:--accent={app.background}>
        <span class="bar"></span>
        <span class="line"></span>
        <span class="line short"></span>
      </span>
    {/each}
  </span>
  <span class="icons">
    {#each stage.apps.slice(0, 3) as app (app.id)}
      <IconTile size={26} background={app.background} shadow>
        <Icon name={app.icon} size={14} weight={2} color="#fff" />
      </IconTile>
    {/each}
  </span>
</button>

<style>
  .thumb {
    appearance: none;
    border: 0;
    background: none;
    padding: 0;
    position: relative;
    width: 108px;
    height: 84px;
    cursor: pointer;
    transition: transform var(--duration-fast) var(--ease-ios);
    transform: perspective(500px) rotateY(14deg);
  }
  .thumb:hover {
    transform: perspective(500px) rotateY(4deg) scale(1.04);
  }
  .thumb:active {
    transform: scale(0.95);
  }
  .cards {
    position: absolute;
    inset: 0;
  }
  .card {
    position: absolute;
    left: calc(var(--i) * 12px);
    top: calc(var(--i) * 8px);
    width: 84px;
    height: 60px;
    border-radius: 8px;
    background: var(--bg-elevated);
    box-shadow:
      0 0 0 0.5px rgb(0 0 0 / 0.12),
      0 6px 16px rgb(0 0 0 / 0.22);
    overflow: hidden;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .bar {
    height: 12px;
    background: var(--accent, var(--fill));
    opacity: 0.85;
  }
  .line {
    height: 4px;
    margin: 0 8px;
    border-radius: 2px;
    background: var(--fill);
  }
  .short {
    width: 50%;
  }
  .icons {
    position: absolute;
    left: -6px;
    bottom: -4px;
    display: flex;
    gap: 2px;
  }
</style>
