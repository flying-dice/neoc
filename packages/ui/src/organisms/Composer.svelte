<!--
  @organism Composer — chat input bar with an optional accessory row above it (e.g. session controls). While a run is
  going the send button becomes Stop. Typing `/` at the start opens the slash-command list (Arrow keys, Enter or Tab
  to pick, Escape to close). Atoms: Material, TextInput. Molecules: IconButton.
  Test ids: `testid` on the form, `${testid}-input`, `${testid}-send`, `${testid}-stop`, `${testid}-attach`,
  `${testid}-commands` (the list) and `${testid}-command-<name>` (an item).
-->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import Material from '../atoms/Material.svelte'
  import TextInput from '../atoms/TextInput.svelte'
  import IconButton from '../molecules/IconButton.svelte'
  import type { ChatCommand } from '../types'

  interface Props {
    testid: string
    value?: string
    placeholder?: string
    disabled?: boolean
    /** A run is in progress: show Stop instead of Send. */
    running?: boolean
    commands?: ChatCommand[]
    accessory?: Snippet
    onsend: (text: string) => void
    onstop?: () => void
  }

  let {
    testid,
    value = $bindable(''),
    placeholder = 'Message',
    disabled = false,
    running = false,
    commands = [],
    accessory,
    onsend,
    onstop
  }: Props = $props()

  let input: HTMLInputElement | undefined = $state()
  let active = $state(0)
  let dismissed = $state(false)

  /** Focus the text field, caret at the end. */
  export function focus(): void {
    input?.focus()
    input?.setSelectionRange(value.length, value.length)
  }

  const matches = $derived.by(() => {
    if (dismissed || !value.startsWith('/') || value.includes(' ')) return []
    const q = value.slice(1).toLowerCase()
    return commands.filter((c) => c.name.toLowerCase().startsWith(q)).slice(0, 8)
  })
  const open = $derived(matches.length > 0)
  const listId = $derived(`${testid}-commands`)

  $effect(() => {
    void value
    active = 0
    dismissed = false
  })

  function pick(c: ChatCommand): void {
    value = `/${c.name} `
    focus()
  }

  function send(): void {
    if (!value.trim() || disabled || running) return
    onsend(value.trim())
    value = ''
  }

  function submit(e: SubmitEvent): void {
    e.preventDefault()
    if (open) return pick(matches[Math.min(active, matches.length - 1)])
    send()
  }

  function keydown(e: KeyboardEvent): void {
    if (!open) return
    if (e.key === 'ArrowDown') active = (active + 1) % matches.length
    else if (e.key === 'ArrowUp') active = (active - 1 + matches.length) % matches.length
    else if (e.key === 'Tab') pick(matches[Math.min(active, matches.length - 1)])
    else if (e.key === 'Escape') dismissed = true
    else return
    e.preventDefault()
  }
</script>

<Material thickness="chrome">
  <div class="surface">
    {#if accessory}{@render accessory()}{/if}
    <!-- Key presses bubble up from the text field: Arrow, Tab and Escape drive the slash-command list. -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <form class="composer" data-testid={testid} onsubmit={submit} onkeydown={keydown}>
      {#if commands.length}
        <!-- Always in the DOM (hidden while closed): the input's `aria-controls` must name an element that exists. -->
        <ul
          class="commands"
          id={listId}
          role="listbox"
          aria-label="Slash commands"
          hidden={!open}
          data-testid={listId}
        >
          {#each matches as c, i (c.name)}
            <li role="presentation">
              <button
                type="button"
                role="option"
                id="{listId}-{c.name}"
                class="command"
                class:active={i === active}
                aria-selected={i === active}
                tabindex="-1"
                data-testid="{testid}-command-{c.name}"
                onclick={() => pick(c)}
              >
                <span class="name">/{c.name}</span>
                <span class="desc">{c.description}</span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
      <IconButton
        testid="{testid}-attach"
        icon="plus"
        label="Attach"
        variant="gray"
        size="small"
        capsule
        hit
        disabled
      />
      <label class="field">
        <TextInput
          testid="{testid}-input"
          bind:ref={input}
          bind:value
          {placeholder}
          aria-label={placeholder}
          role={commands.length ? 'combobox' : undefined}
          aria-controls={commands.length ? listId : undefined}
          aria-expanded={commands.length ? open : undefined}
          aria-autocomplete={commands.length ? 'list' : undefined}
          aria-activedescendant={open ? `${listId}-${matches[Math.min(active, matches.length - 1)].name}` : undefined}
          {disabled}
        />
      </label>
      {#if running}
        <IconButton
          testid="{testid}-stop"
          icon="stop"
          label="Stop"
          variant="filled"
          size="small"
          capsule
          hit
          onclick={() => onstop?.()}
        />
      {:else}
        <IconButton
          testid="{testid}-send"
          icon="arrow.up"
          label="Send"
          variant="filled"
          size="small"
          capsule
          hit
          disabled={!value.trim() || disabled}
          onclick={send}
        />
      {/if}
    </form>
  </div>
</Material>

<style>
  .surface {
    box-shadow: 0 calc(var(--hairline) * -1) 0 var(--separator);
  }
  .composer {
    position: relative;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 12px 8px;
  }
  .field {
    flex: 1;
    display: flex;
    min-width: 0;
    min-height: 34px;
    align-items: center;
    padding: 0 14px;
    border-radius: 17px;
    box-shadow: inset 0 0 0 1px var(--separator-opaque);
    background: var(--bg-elevated);
  }
  .commands[hidden] {
    display: none;
  }
  .commands {
    position: absolute;
    left: 12px;
    right: 12px;
    bottom: calc(100% + 4px);
    margin: 0;
    padding: 4px;
    list-style: none;
    max-height: 220px;
    overflow-y: auto;
    border-radius: 14px;
    background: var(--bg-elevated);
    box-shadow:
      0 0 0 1px var(--separator),
      var(--shadow-popover, 0 8px 24px rgb(0 0 0 / 0.18));
    z-index: 2;
  }
  .command {
    appearance: none;
    border: 0;
    width: 100%;
    display: flex;
    gap: 10px;
    align-items: baseline;
    padding: 7px 10px;
    border-radius: 10px;
    background: transparent;
    color: var(--label);
    font: var(--type-subheadline);
    text-align: left;
    cursor: pointer;
  }
  .command.active,
  .command:hover {
    background: var(--fill-tertiary);
  }
  .name {
    font-weight: 600;
    flex: none;
  }
  .desc {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--label-secondary);
    font: var(--type-footnote);
  }
</style>
