<!-- @atom SplitHandle — the Split View divider grip; reports horizontal drag deltas. Test id: `testid`. -->
<script lang="ts">
  interface Props {
    testid: string
    /** Pointer x delta since the drag began. */
    ondrag?: (dx: number) => void
    ondragend?: () => void
  }

  let { testid, ondrag, ondragend }: Props = $props()

  let startX: number | null = $state(null)

  function down(e: PointerEvent): void {
    if (e.button !== 0) return
    ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
    startX = e.clientX
  }
  function move(e: PointerEvent): void {
    if (startX !== null) ondrag?.(e.clientX - startX)
  }
  function up(): void {
    if (startX === null) return
    startX = null
    ondragend?.()
  }
</script>

<div
  class="split-handle"
  class:active={startX !== null}
  role="separator"
  data-testid={testid}
  aria-orientation="vertical"
  aria-label="Resize split"
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={up}
>
  <span class="grip"></span>
</div>

<style>
  .split-handle {
    width: 10px;
    height: 100%;
    display: grid;
    place-items: center;
    cursor: col-resize;
    touch-action: none;
  }
  .grip {
    width: 5px;
    height: 44px;
    border-radius: 3px;
    background: rgb(255 255 255 / 0.7);
    box-shadow: 0 0 0 0.5px rgb(0 0 0 / 0.15);
    transition:
      height var(--duration-fast) var(--ease-ios),
      background-color var(--duration-fast);
  }
  .split-handle:hover .grip,
  .active .grip {
    height: 64px;
    background: #fff;
  }
</style>
