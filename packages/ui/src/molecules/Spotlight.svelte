<!--
  @molecule Spotlight — the panel of system search, as iPadOS Spotlight: a search field centred near the top of a dimmed,
  blurred screen, the results under it. It fills the positioned box it is put in (a layer over the whole screen). What
  is searched, and what a result does, are the owner's: this is the field, the list and the keys.
  Atoms: Material, Icon, IconTile, TextInput.
  Test ids: `testid` on the dialog, `${testid}-scrim`, `${testid}-field`, `${testid}-clear`, `${testid}-results`,
  `${testid}-result-<item id>` on each result, `${testid}-empty`, `${testid}-status` (the live region).

  Keyboard: the field has focus from the moment the panel opens and keeps it (the results are never focused: the
  highlighted one is the field's `aria-activedescendant`). Up and Down ask the owner to move the highlight (`onmove`),
  Enter opens the highlighted result (`onactivate`), Escape clears a query, and with none leaves (`ondismiss`); a press
  on the dimmed screen leaves too. Tab stays inside the panel. No key pressed here reaches anything behind it.
  When the panel goes away focus returns to where it was (the owner moves it on when a result opened something); it
  goes at once, with no closing transition, so that happens before the owner's next step.

  To assistive technology it is a modal dialog holding a combobox (the field) that controls a listbox (the results);
  `status` ("3 results", "No results") is said politely when it changes.

  With Reduce Transparency the screen is dimmed, not blurred, and the panel is solid (the material tokens).
-->
<script lang="ts">
  import { fade } from 'svelte/transition'
  import Material from '../atoms/Material.svelte'
  import Icon from '../atoms/Icon.svelte'
  import IconTile from '../atoms/IconTile.svelte'
  import TextInput from '../atoms/TextInput.svelte'
  import { trapFocus } from '../focus'
  import { motion } from '../motion'
  import type { SpotlightItem } from '../types'

  interface Props {
    testid: string
    query?: string
    items: SpotlightItem[]
    /** The id of the highlighted result: the one Enter opens. */
    activeId?: string | null
    /** A line over the results that says what they are when nothing was asked for ("Suggestions"). */
    heading?: string
    /** What a screen reader is told about the results ("3 results"). */
    status?: string
    label?: string
    placeholder?: string
    /** Up or Down: highlight the previous or next result. */
    onmove?: (delta: 1 | -1) => void
    /** The pointer is on a result: highlight it. */
    onhighlight?: (id: string) => void
    /** Enter, or a click on a result. */
    onactivate?: (id: string) => void
    /** Escape with nothing typed, or a press outside the panel. */
    ondismiss?: () => void
  }

  let {
    testid,
    query = $bindable(''),
    items,
    activeId = null,
    heading,
    status = '',
    label = 'Search',
    placeholder = 'Search',
    onmove,
    onhighlight,
    onactivate,
    ondismiss
  }: Props = $props()

  let input: HTMLInputElement | undefined = $state()
  let list: HTMLElement | undefined = $state()

  const optionId = (id: string): string => `${testid}-result-${id}`
  const active = $derived(items.some((i) => i.id === activeId) ? activeId : null)

  // A live region that is inserted with its text already in it is often not announced: the text is set after it is there.
  let said = $state('')
  $effect(() => {
    said = status
  })

  // the highlighted result is kept in view while the arrows move through a list that scrolls
  $effect(() => {
    if (active === null || !list) return
    document.getElementById(optionId(active))?.scrollIntoView({ block: 'nearest' })
  })

  function onkeydown(e: KeyboardEvent): void {
    e.stopPropagation() // nothing typed here is a key for the window or the desktop behind
    if (e.isComposing) return
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      onmove?.(e.key === 'ArrowDown' ? 1 : -1)
    } else if (e.key === 'Enter') {
      e.preventDefault()
      if (active !== null) onactivate?.(active)
    } else if (e.key === 'Escape') {
      e.preventDefault()
      if (query) query = ''
      else ondismiss?.()
    }
  }

  function clear(): void {
    query = ''
    input?.focus()
  }
</script>

<div class="ios-spotlight-layer">
  <div
    class="scrim"
    role="presentation"
    data-testid="{testid}-scrim"
    onpointerdown={(e) => {
      e.preventDefault() // the press only leaves search: focus is put back by the panel as it goes
      ondismiss?.()
    }}
    in:fade|global={{ duration: motion(180) }}
  ></div>
  <div
    class="panel"
    role="dialog"
    tabindex="-1"
    aria-modal="true"
    aria-label={label}
    data-testid={testid}
    {onkeydown}
    {@attach trapFocus()}
    in:fade|global={{ duration: motion(180) }}
  >
    <Material thickness="thick" radius="var(--radius-xl)">
      <label class="field">
        <Icon name="magnifyingglass" size={22} weight={2.2} />
        <TextInput
          testid="{testid}-field"
          bind:ref={input}
          bind:value={query}
          class="ios-spotlight-input"
          type="text"
          role="combobox"
          {placeholder}
          aria-label={label}
          aria-autocomplete="list"
          aria-expanded={items.length > 0}
          aria-controls="{testid}-results"
          aria-activedescendant={active === null ? undefined : optionId(active)}
          autocomplete="off"
          autocapitalize="off"
          autocorrect="off"
          spellcheck={false}
          enterkeyhint="go"
          data-autofocus
        />
        {#if query}
          <button type="button" class="clear" aria-label="Clear" data-testid="{testid}-clear" onclick={clear}>
            <Icon name="xmark" size={11} weight={3.2} />
          </button>
        {/if}
      </label>

      {#if items.length > 0 && heading}
        <div class="heading" id="{testid}-heading">{heading}</div>
      {/if}
      <!-- always there, so the field's `aria-controls` always names something -->
      <ul
        class="results"
        class:none={items.length === 0}
        role="listbox"
        id="{testid}-results"
        aria-label={heading ?? 'Results'}
        data-testid="{testid}-results"
        bind:this={list}
      >
        {#each items as item (item.id)}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus (focus stays in the field: Enter there opens the highlighted result) -->
          <li
            class="result"
            class:active={item.id === active}
            role="option"
            id={optionId(item.id)}
            aria-selected={item.id === active}
            data-testid={optionId(item.id)}
            onpointermove={() => item.id !== active && onhighlight?.(item.id)}
            onpointerdown={(e) => e.preventDefault()}
            onclick={() => onactivate?.(item.id)}
          >
            <IconTile size={32} background={item.background}>
              <Icon name={item.icon} size={18} weight={2} color="#fff" />
            </IconTile>
            <span class="title">{item.title}</span>
            {#if item.kind}<span class="kind">{item.kind}</span>{/if}
          </li>
        {/each}
      </ul>
      {#if items.length === 0 && query.trim()}
        <p class="empty" data-testid="{testid}-empty">No Results</p>
      {/if}
    </Material>
    <div class="sr-only" role="status" aria-live="polite" data-testid="{testid}-status">{said}</div>
  </div>
</div>

<style>
  .ios-spotlight-layer {
    position: absolute;
    inset: 0;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding: min(14%, 120px) 16px 16px;
    pointer-events: auto;
  }
  .scrim {
    position: absolute;
    inset: 0;
    background: var(--dimming);
    -webkit-backdrop-filter: var(--material-blur);
    backdrop-filter: var(--material-blur);
  }
  .panel {
    position: relative;
    width: min(640px, 100%);
    max-height: 100%;
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-xl);
    box-shadow: var(--shadow-popover);
    color: var(--label);
    overflow: hidden;
  }
  .panel > :global(.ios-material) {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .field {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    height: 56px;
    padding: 0 16px;
    color: var(--label-secondary);
    cursor: text;
  }
  .field :global(.ios-spotlight-input) {
    font: var(--type-title3);
    letter-spacing: var(--tracking-title3);
  }
  .clear {
    position: relative;
    flex: none;
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: var(--label-secondary);
    color: var(--bg-elevated);
    cursor: pointer;
  }
  /* a 20 px glyph with the 44 px target the design language asks for */
  .clear::before {
    content: '';
    position: absolute;
    inset: -12px;
  }
  .heading {
    flex: none;
    padding: 10px 16px 4px;
    box-shadow: 0 calc(var(--hairline) * -1) 0 var(--separator);
    color: var(--label-secondary);
    font: var(--type-footnote);
    font-weight: 600;
    letter-spacing: var(--tracking-footnote);
  }
  .results {
    list-style: none;
    margin: 0;
    padding: 6px 0;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    box-shadow: 0 calc(var(--hairline) * -1) 0 var(--separator);
  }
  .heading + .results {
    box-shadow: none;
    padding-top: 2px;
  }
  .results.none {
    display: none;
  }
  .result {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 48px;
    padding: 0 var(--row-inset);
    cursor: pointer;
  }
  .title {
    flex: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .kind {
    flex: none;
    color: var(--label-secondary);
    font: var(--type-subheadline);
    letter-spacing: var(--tracking-subheadline);
  }
  /* the highlighted result: white on the accent's fill, which holds 4.5:1 for every accent in both themes */
  .result.active {
    background: var(--tint-fill);
    color: #fff;
  }
  .result.active .kind {
    color: #fff;
  }
  .empty {
    margin: 0;
    padding: 22px 16px 24px;
    box-shadow: 0 calc(var(--hairline) * -1) 0 var(--separator);
    color: var(--label-secondary);
    font: var(--type-title3);
    letter-spacing: var(--tracking-title3);
    text-align: center;
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
