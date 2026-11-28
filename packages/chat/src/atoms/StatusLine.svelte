<!-- @atom StatusLine — the line over the composer while the agent's turn runs, as a typing indicator: a moving glyph in the accent and what the agent is at; a polite live region that is empty the rest of the time -->
<script lang="ts">
  interface Props {
    /** What the agent is at (`Thinking… · Run the tests`); null when no turn runs. */
    text: string | null
    /** The app's word, which the test ids start with: `agents` gives `agents-status`. */
    testid: string
  }

  let { text, testid }: Props = $props()
</script>

<!-- always there, so a screen reader hears the words when they come and change, not only once it notices the region -->
<div class="status" class:shown={text !== null} role="status" aria-live="polite" data-testid="{testid}-status">
  {#if text !== null}
    <span class="glyph" aria-hidden="true"><i></i><i></i><i></i></span>
    <span class="text">{text}</span>
  {/if}
</div>

<style>
  .status {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    padding: 0 16px;
    font: var(--type-subheadline);
    color: var(--label-secondary);
  }
  .shown {
    padding-block: 6px 2px;
  }
  .text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* three dots in the accent that take turns to swell: the agent is at work */
  .glyph {
    flex: none;
    position: relative;
    width: 14px;
    height: 14px;
  }
  .glyph i {
    position: absolute;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--tint);
    animation: swell 1.2s ease-in-out infinite;
  }
  .glyph i:nth-child(1) {
    left: 0;
    top: 1px;
  }
  .glyph i:nth-child(2) {
    right: 0;
    top: 3px;
    animation-delay: 0.4s;
  }
  .glyph i:nth-child(3) {
    left: 4px;
    bottom: 0;
    animation-delay: 0.8s;
  }
  @keyframes swell {
    0%,
    100% {
      transform: scale(0.6);
      opacity: 0.45;
    }
    40% {
      transform: scale(1.15);
      opacity: 1;
    }
  }
  /* with less motion asked for, the dots stand still */
  @media (prefers-reduced-motion: reduce) {
    .glyph i {
      animation: none;
      transform: none;
      opacity: 1;
    }
  }
</style>
