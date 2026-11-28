<!-- @molecule CopyButton — the button that puts everything known of a log or a trace on the clipboard, and says how that went -->
<script lang="ts">
  import type { Copied } from '../lib/telemetry.svelte'

  interface Props {
    /** How the last copy went. */
    copied?: Copied
    /** Nothing to copy yet (a trace still being read). */
    disabled?: boolean
    oncopy: () => void
    testid: string
  }

  let { copied = 'idle', disabled = false, oncopy, testid }: Props = $props()

  const SAID = {
    idle: '',
    copied: 'Everything about it is on the clipboard.',
    refused: 'The clipboard did not take it: this window may not write to it.'
  } as const
</script>

<div class="copy">
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <ui-button variant="tinted" {disabled} onclick={oncopy} data-testid={testid}>
    <ui-icon slot="icon" name={copied === 'copied' ? 'checkmark' : 'doc.on.doc'} size="16"></ui-icon>
    {copied === 'copied' ? 'Copied' : 'Copy Details'}
  </ui-button>
  <!-- Said as it changes, to people who cannot see the button's label change. A copy that worked is quiet to the eye (the
       label and the checkmark say so, FEED-01) and takes no room; one that failed is written out under the button. -->
  <p class:ios-footnote={copied === 'refused'} class:ios-red={copied === 'refused'} class:said={copied === 'copied'} role="status" data-testid="{testid}-status">{SAID[copied]}</p>
</div>

<style>
  .copy {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
  }
  p {
    margin: 0;
  }
  /* seen by assistive technology only */
  .said,
  p:empty {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
