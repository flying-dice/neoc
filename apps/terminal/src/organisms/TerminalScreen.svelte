<!-- @organism TerminalScreen — the terminal emulator (xterm.js): it draws the bytes it is given, and says what was typed and what size it has -->
<script lang="ts">
  import { FitAddon } from '@xterm/addon-fit'
  import { Terminal as Xterm } from '@xterm/xterm'
  import '@xterm/xterm/css/xterm.css'
  import type { Screen } from '../lib/terminal.svelte'

  interface Props {
    /** Whether typing is taken (there is a shell to type to). */
    interactive: boolean
    /** The emulator is on the page, at this size: draw into `screen`. */
    onready: (screen: Screen, rows: number, cols: number) => void
    /** What was typed or pasted, as text. */
    ondata: (text: string) => void
    /** Typed bytes that are not text (a mouse report in an old encoding). */
    onbinary: (bytes: Uint8Array) => void
    /** The emulator now has this many rows and columns. */
    onresize: (rows: number, cols: number) => void
  }

  let { interactive, onready, ondata, onbinary, onresize }: Props = $props()

  // The sixteen colours a program asks for by number, each legible (4.5:1 at least) on the background: the emulator's own
  // red and blue are not, on a dark screen. They are the system colours of a dark iPad; "black" is a grey, as text.
  const THEME = {
    background: '#1c1c1e',
    foreground: '#f2f2f7',
    cursor: '#f2f2f7',
    selectionBackground: '#3a3a3c',
    black: '#98989d',
    red: '#ff6961',
    green: '#32d74b',
    yellow: '#ffd60a',
    blue: '#409cff',
    magenta: '#da8fff',
    cyan: '#64d2ff',
    white: '#f2f2f7',
    brightBlack: '#aeaeb2',
    brightRed: '#ff8a84',
    brightGreen: '#5ee272',
    brightYellow: '#ffe04d',
    brightBlue: '#70b5ff',
    brightMagenta: '#e5b0ff',
    brightCyan: '#8fdfff',
    brightWhite: '#ffffff'
  }

  let term: Xterm | undefined

  /** Give the emulator the keyboard. */
  /** Take the keyboard, when the page has it: a frame may not focus itself before the person has acted in it. */
  export function focus(): void {
    if (document.hasFocus()) term?.focus()
  }

  // the emulator lives as long as the element; the callbacks are read when they are called, so a new one is used at once
  function emulator(element: HTMLElement): () => void {
    const style = getComputedStyle(element)
    const xterm = new Xterm({
      // dark whatever the appearance, as a terminal is; a steady cursor (a blinking one never looks the same twice)
      cursorBlink: false,
      fontFamily: style.getPropertyValue('--font-mono').trim() || 'ui-monospace, Menlo, Consolas, monospace',
      fontSize: 13,
      scrollback: 5000,
      theme: THEME
    })
    const fit = new FitAddon()
    xterm.loadAddon(fit)
    xterm.open(element)
    fit.fit()
    term = xterm

    const subscriptions = [
      xterm.onData((text) => ondata(text)),
      xterm.onBinary((binary) => onbinary(Uint8Array.from(binary, (char) => char.charCodeAt(0)))),
      xterm.onResize(({ rows, cols }) => onresize(rows, cols))
    ]
    // the window's size is the terminal's: fit again whenever the element changes
    const observer = new ResizeObserver(() => fit.fit())
    observer.observe(element)

    onready({ write: (bytes, drawn) => xterm.write(bytes, drawn), reset: () => xterm.reset() }, xterm.rows, xterm.cols)
    if (document.hasFocus()) xterm.focus()

    return () => {
      observer.disconnect()
      subscriptions.forEach((subscription) => subscription.dispose())
      xterm.dispose()
      term = undefined
    }
  }

  $effect(() => {
    if (term) term.options.disableStdin = !interactive
  })
</script>

<div class="screen" data-testid="terminal-screen" {@attach emulator}></div>

<style>
  .screen {
    box-sizing: border-box;
    width: 100%;
    height: 100%;
    min-height: 0;
    overflow: hidden;
    background: #1c1c1e;
  }
  /* the emulator fills the element; its own scrollbar is its scrollback */
  .screen :global(.xterm) {
    height: 100%;
  }
  /* the emulator's own sheet paints the strip under the last whole row black: it is part of the screen */
  .screen :global(.xterm .xterm-viewport) {
    background-color: #1c1c1e;
  }
</style>
