<!--
  @organism StageStrip — Stage Manager's recent-apps strip down the left edge.
  When `autoHide` is set (a window overlaps the strip, or one is full screen)
  it slides away and returns while the pointer rests on the left edge.
  Molecules: StageThumbnail.
  Test ids: `testid` on the strip, `${testid}-<app ids joined by +>` on each
  thumbnail (e.g. `stage-strip-calculator`, `stage-strip-files+terminal`), `${testid}-edge`.
-->
<script lang="ts">
  import StageThumbnail from '../molecules/StageThumbnail.svelte'
  import type { StageSummary } from '../types'

  interface Props {
    testid: string
    stages: StageSummary[]
    autoHide?: boolean
    onselect?: (id: string) => void
  }

  let { testid, stages, autoHide = false, onselect }: Props = $props()
  let peeking = $state(false)
</script>

{#if autoHide}
  <div class="edge" role="presentation" data-testid="{testid}-edge" onpointerenter={() => (peeking = true)}></div>
{/if}

<nav
  class="strip"
  class:hidden={autoHide && !peeking}
  aria-label="Recent apps"
  data-testid={testid}
  onpointerleave={() => (peeking = false)}
>
  {#each stages as stage (stage.id)}
    <div class="item">
      <StageThumbnail testid="{testid}-{stage.apps.map((a) => a.id).join('+')}" {stage} onclick={() => onselect?.(stage.id)} />
    </div>
  {/each}
</nav>

<style>
  .strip {
    position: absolute;
    top: 0;
    bottom: 0;
    left: 0;
    z-index: 700;
    width: 150px;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 28px;
    padding: 20px 0 110px 26px;
    transition:
      transform var(--duration-slow) var(--ease-ios),
      opacity var(--duration-slow) var(--ease-ios);
  }
  .hidden {
    transform: translateX(-120%);
    opacity: 0;
    pointer-events: none;
  }
  .edge {
    position: absolute;
    top: 0;
    bottom: 0;
    left: 0;
    width: 10px;
    z-index: 701;
  }
  .item {
    display: flex;
  }
</style>
