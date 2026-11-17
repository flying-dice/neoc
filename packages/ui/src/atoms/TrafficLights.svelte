<!-- @atom TrafficLights — macOS close / minimize / zoom window buttons.
  Test ids: `testid` on the group, `${testid}-close|minimize|zoom` on the buttons. -->
<script lang="ts">
  interface Props {
    testid: string
    /** Inactive windows show gray lights. */
    active?: boolean
    onclose?: () => void
    onminimize?: () => void
    onzoom?: () => void
  }

  let { testid, active = true, onclose, onminimize, onzoom }: Props = $props()
</script>

<div class="lights" class:active role="group" aria-label="Window controls" data-testid={testid}>
  <button type="button" class="light close" aria-label="Close" data-testid="{testid}-close" onclick={onclose}>
    <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3.8 3.8l4.4 4.4M8.2 3.8L3.8 8.2" /></svg>
  </button>
  <button type="button" class="light minimize" aria-label="Minimize" data-testid="{testid}-minimize" onclick={onminimize}>
    <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3 6h6" /></svg>
  </button>
  <button type="button" class="light zoom" aria-label="Zoom" data-testid="{testid}-zoom" onclick={onzoom}>
    <svg viewBox="0 0 12 12" aria-hidden="true">
      <path d="M3.5 7.8V3.5h4.3z M8.5 4.2v4.3H4.2z" fill="currentColor" stroke="none" />
    </svg>
  </button>
</div>

<style>
  .lights {
    display: inline-flex;
    gap: 8px;
  }
  .light {
    appearance: none;
    width: 12px;
    height: 12px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    display: grid;
    place-items: center;
    cursor: default;
    background: var(--ios-gray4);
    box-shadow: inset 0 0 0 0.5px rgb(0 0 0 / 0.12);
    color: rgb(0 0 0 / 0.55);
  }
  .active .close {
    background: #ff5f57;
    box-shadow: inset 0 0 0 0.5px #e0443e;
  }
  .active .minimize {
    background: #febc2e;
    box-shadow: inset 0 0 0 0.5px #dea123;
  }
  .active .zoom {
    background: #28c840;
    box-shadow: inset 0 0 0 0.5px #1aab29;
  }
  /* Hovering the group reveals glyphs on all three, even when inactive. */
  .lights:hover .close {
    background: #ff5f57;
  }
  .lights:hover .minimize {
    background: #febc2e;
  }
  .lights:hover .zoom {
    background: #28c840;
  }
  svg {
    width: 8px;
    height: 8px;
    opacity: 0;
    stroke: currentColor;
    stroke-width: 1.4;
    stroke-linecap: round;
    fill: none;
  }
  .zoom svg {
    width: 10px;
    height: 10px;
  }
  .lights:hover svg {
    opacity: 1;
  }
  .light:active {
    filter: brightness(0.85);
  }
</style>
