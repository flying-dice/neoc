<!--
  @molecule Sheet — modal sheet presentation (iPadOS form sheet / iOS page sheet).
  Atoms: Scrim, Grabber.
  Test ids: `testid` on the sheet, `${testid}-scrim`.
-->
<script lang="ts">
  import { motion } from '../motion'
  import type { Snippet } from 'svelte'
  import { fly } from 'svelte/transition'
  import { cubicOut } from 'svelte/easing'
  import Scrim from '../atoms/Scrim.svelte'
  import Grabber from '../atoms/Grabber.svelte'

  interface Props {
    testid: string
    open?: boolean
    presentation?: 'form' | 'page'
    width?: number
    /** Tap on the dimmed backdrop / Escape dismisses. */
    dismissible?: boolean
    showGrabber?: boolean
    label?: string
    onclose?: () => void
    children: Snippet
  }

  let {
    testid,
    open = $bindable(false),
    presentation = 'form',
    width = 540,
    dismissible = true,
    showGrabber = false,
    label,
    onclose,
    children
  }: Props = $props()

  function close(): void {
    open = false
    onclose?.()
  }

  function onkeydown(e: KeyboardEvent): void {
    if (open && dismissible && e.key === 'Escape') close()
  }
</script>

<svelte:window {onkeydown} />

{#if open}
  <div class="ios-sheet-layer {presentation}">
    <Scrim testid="{testid}-scrim" onclick={() => dismissible && close()} />
    <div
      class="sheet"
      role="dialog"
      data-testid={testid}
      aria-modal="true"
      aria-label={label}
      style:--sheet-width="{width}px"
      transition:fly={{ y: 700, duration: motion(450), easing: cubicOut, opacity: 1 }}
    >
      {#if showGrabber}<div class="grabber"><Grabber /></div>{/if}
      {@render children()}
    </div>
  </div>
{/if}

<style>
  .ios-sheet-layer {
    position: absolute;
    inset: 0;
    z-index: 1000;
    display: flex;
    justify-content: center;
  }
  .form {
    align-items: center;
  }
  .page {
    align-items: flex-end;
  }
  .sheet {
    position: relative;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--bg-grouped-elevated);
    color: var(--label);
    box-shadow: var(--shadow-popover);
  }
  .form .sheet {
    width: min(var(--sheet-width), calc(100% - 40px));
    max-height: min(620px, calc(100% - 80px));
    border-radius: 13px;
  }
  .page .sheet {
    width: 100%;
    max-height: calc(100% - 44px);
    border-radius: 13px 13px 0 0;
  }
  .grabber {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    z-index: 1;
  }
</style>
