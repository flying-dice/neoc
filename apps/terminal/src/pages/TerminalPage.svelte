<!-- @page TerminalPage — the whole terminal: the emulator, and under it a line when the session is not simply running -->
<script lang="ts">
  import SessionBar from '../molecules/SessionBar.svelte'
  import TerminalScreen from '../organisms/TerminalScreen.svelte'
  import type { Screen, Terminal } from '../lib/terminal.svelte'

  interface Props {
    /** This window's session; the composition root builds it with the real backend, a story with a fake. */
    terminal: Terminal
  }

  let { terminal }: Props = $props()

  let screen = $state<ReturnType<typeof TerminalScreen>>()

  // hear the backend while the page is open
  $effect(() => terminal.listen())

  // a session opens when the page starts, at the size the emulator took
  function ready(drawn: Screen, rows: number, cols: number): void {
    terminal.attach(drawn)
    void terminal.start(rows, cols)
  }

  async function restart(): Promise<void> {
    await terminal.restart()
    screen?.focus()
  }

  /** How the shell ended, as a sentence. */
  const ended = $derived.by(() => {
    const exit = terminal.exit
    if (exit === null) return 'The shell exited.'
    if (exit.signal !== undefined) return `The shell was ended (${exit.signal}).`
    return exit.code === 0 ? 'The shell exited.' : `The shell exited with code ${exit.code}.`
  })
</script>

<!-- the terminal takes the keyboard whenever its window does -->
<svelte:window onfocus={() => screen?.focus()} />

<main class="terminal" data-theme="dark" data-testid="terminal-app" data-phase={terminal.phase}>
  <div class="emulator">
    <TerminalScreen
      bind:this={screen}
      interactive={terminal.phase === 'running'}
      onready={ready}
      ondata={(text) => terminal.type(text)}
      onbinary={(bytes) => terminal.typeBytes(bytes)}
      onresize={(rows, cols) => terminal.resize(rows, cols)}
    />
  </div>
  {#if terminal.phase === 'starting'}
    <SessionBar message="Starting the shell…" busy />
  {:else if terminal.phase === 'failed'}
    <SessionBar message={terminal.failure ?? 'The shell could not be started.'} action="Try Again" onaction={restart} />
  {:else if terminal.phase === 'exited'}
    <SessionBar message={ended} action="New Session" onaction={restart} />
  {:else if terminal.phase === 'ended'}
    <SessionBar message="This session has ended: the window was away for too long." action="New Session" onaction={restart} />
  {:else if terminal.problem !== null}
    <SessionBar message={`${terminal.problem} What you type is sent when the connection is back.`} />
  {/if}
</main>

<style>
  /* dark whatever the appearance, as a terminal is: the kit's tokens follow this data-theme */
  .terminal {
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    height: 100%;
    background: #1c1c1e;
    color: var(--label);
    color-scheme: dark;
  }
  .emulator {
    flex: 1 1 auto;
    min-height: 0;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding: 28px 4px 4px 16px;
  }
</style>
