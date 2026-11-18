<!--
  @molecule ListRow — UITableViewCell / list cell. Atoms: IconTile, Icon. Test ids: `testid` on the row,
  `${testid}-tap` on the tap target and `${testid}-handle` on the reorder handle of a managed row.

  Two forms. Without `role` it is the plain cell the shell uses: a `<button>` when it has `onclick`.
  With `role` (`listitem` or `option`) it is a managed row inside a `List`:
   - a tappable `listitem` is a `<div>` with an invisible full-row `<button>` (named from the title) laid over its
     content, so interactive `trailing` content is a sibling, not a nested button;
   - an `option` (a selectable list) is the focusable element itself, with `aria-selected`;
   - `aria-posinset` / `aria-setsize` say "7 of 4 312" even though only a window of rows is in the DOM;
   - `swipeLeading` / `swipeTrailing` reveal actions by dragging; the same actions open from the keyboard
     (`Alt+Arrow`, the menu key) and stay in the accessibility tree;
   - `reorder` shows a drag handle (reordering by keyboard is the list's `Alt+Arrow`);
   - `placeholder` is a loading skeleton that keeps the row's geometry.
-->
<script lang="ts">
  import { tick, type Snippet } from 'svelte'
  import Icon from '../atoms/Icon.svelte'
  import IconTile from '../atoms/IconTile.svelte'
  import type { IconName } from '../icons'
  import {
    isHorizontalDrag,
    restingOffset,
    swipeOffset,
    swipeSettle,
    type SwipeGeometry,
    type SwipeOpen,
    type SwipeSide
  } from '../logic/gestures'

  interface Props {
    testid: string
    title: string
    subtitle?: string
    /** Right-aligned secondary value (e.g. "On", a network name). */
    value?: string
    /** Settings-style rounded glyph tile. */
    icon?: IconName
    iconColor?: string
    chevron?: boolean
    selected?: boolean
    destructive?: boolean
    /** Tinted "action" row (e.g. "Add Account…"). */
    action?: boolean
    onclick?: () => void
    leading?: Snippet
    trailing?: Snippet
    /** Custom content in place of the title and subtitle. */
    children?: Snippet

    // ── managed rows ──
    role?: 'listitem' | 'option'
    /** The row's key (reordering, focus and scroll-to address rows by it). */
    rowKey?: string | number
    posinset?: number
    setsize?: number
    /** Accessible name when it should differ from the title. */
    label?: string
    disabled?: boolean
    /** Reveal actions by swiping right / left. Snippets of `SwipeAction`s. */
    swipeLeading?: Snippet
    swipeTrailing?: Snippet
    /** A full swipe runs the outermost action: told here, done by the caller. */
    fullSwipe?: { leading?: boolean; trailing?: boolean }
    onfullswipe?: (side: SwipeSide) => void
    /** Which actions are open (a bindable so the list can open them from the keyboard). */
    open?: SwipeOpen
    reorder?: boolean
    placeholder?: boolean
    /** Multiple selection: a check circle at the row's start instead of a filled row. */
    check?: boolean
  }

  let {
    testid,
    title,
    subtitle,
    value,
    icon,
    iconColor = 'var(--ios-gray)',
    chevron = false,
    selected = false,
    destructive = false,
    action = false,
    onclick,
    leading,
    trailing,
    children,
    role,
    rowKey,
    posinset,
    setsize,
    label,
    disabled = false,
    swipeLeading,
    swipeTrailing,
    fullSwipe,
    onfullswipe,
    open = $bindable('none'),
    reorder = false,
    placeholder = false,
    check
  }: Props = $props()

  const interactive = $derived(!!onclick)
  const managed = $derived(role !== undefined)
  const option = $derived(role === 'option')
  const tappable = $derived(!!onclick && !disabled && !placeholder)
  const name = $derived(label ?? (subtitle ? `${title}, ${subtitle}` : title))
  const hasSwipe = $derived(!!swipeLeading || !!swipeTrailing)

  // ── swipe ──
  let leadingEl: HTMLElement | undefined = $state()
  let trailingEl: HTMLElement | undefined = $state()
  let faceEl: HTMLElement | undefined = $state()
  let dragging = $state(false)
  let offset = $state(0)
  const geometry = $derived<SwipeGeometry>({
    leading: swipeLeading ? (leadingEl?.offsetWidth ?? 0) : 0,
    trailing: swipeTrailing ? (trailingEl?.offsetWidth ?? 0) : 0,
    row: faceEl?.offsetWidth ?? 0,
    fullSwipe: { leading: !!fullSwipe?.leading, trailing: !!fullSwipe?.trailing }
  })
  // The resting position follows `open`, which the list also sets from the keyboard.
  $effect(() => {
    if (!dragging) offset = restingOffset(open, geometry)
  })

  let press: { x: number; y: number; startOffset: number; t: number; lastX: number; lastT: number; v: number } | null =
    null
  let swiping = false
  let suppressClick = false

  function down(e: PointerEvent): void {
    if (!hasSwipe || e.button !== 0 || (e.target as HTMLElement).closest('[data-no-swipe]')) return
    press = { x: e.clientX, y: e.clientY, startOffset: offset, t: e.timeStamp, lastX: e.clientX, lastT: e.timeStamp, v: 0 }
    swiping = false
  }
  function move(e: PointerEvent): void {
    if (!press) return
    const dx = e.clientX - press.x
    if (!swiping) {
      if (!isHorizontalDrag(dx, e.clientY - press.y)) return
      swiping = true
      dragging = true
      ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
    }
    const dt = Math.max(1, e.timeStamp - press.lastT)
    press.v = (e.clientX - press.lastX) / dt
    press.lastX = e.clientX
    press.lastT = e.timeStamp
    offset = swipeOffset(press.startOffset, dx, geometry)
  }
  function up(): void {
    if (!press) return
    const velocity = press.v
    press = null
    if (!swiping) return
    swiping = false
    dragging = false
    suppressClick = true // the pointer-up that ends a swipe must not tap the row
    queueMicrotask(() => setTimeout(() => (suppressClick = false), 0))
    const settle = swipeSettle(offset, velocity, geometry)
    open = settle.open
    offset = restingOffset(settle.open, geometry)
    if (settle.full) onfullswipe?.(settle.full)
  }
  function guard(e: MouseEvent): void {
    if (suppressClick) {
      e.preventDefault()
      e.stopPropagation()
    }
  }

  // The list opens and closes swipe actions from the keyboard by telling the row (`swipeopen`).
  let rootEl: HTMLElement | undefined = $state()
  $effect(() => {
    const el = rootEl
    if (!el) return undefined
    const swipeOpen = (e: Event): void => {
      const side = (e as CustomEvent<SwipeOpen | 'close'>).detail
      if (side === 'close' || side === 'none') {
        open = 'none'
        el.querySelector<HTMLElement>('[data-row-focus]')?.focus()
        return
      }
      open = side
      void tick().then(() => el.querySelector<HTMLElement>(`[data-actions="${side}"] [data-swipe-action]`)?.focus())
    }
    el.addEventListener('swipeopen', swipeOpen)
    return () => el.removeEventListener('swipeopen', swipeOpen)
  })

  function optionKey(e: KeyboardEvent): void {
    if (e.target !== e.currentTarget || disabled) return
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault()
      onclick?.()
    }
  }
</script>

{#snippet body()}
  {#if children}
    <span class="custom">{@render children()}</span>
  {:else}
    <span class="text" aria-hidden={managed && tappable ? 'true' : undefined}>
      <span class="title" class:destructive class:action>{title}</span>
      {#if subtitle}<span class="subtitle">{subtitle}</span>{/if}
    </span>
  {/if}
{/snippet}

{#snippet accessory()}
  <span class="accessory">
    {#if value}<span class="value">{value}</span>{/if}
    {#if trailing}<span class="trail">{@render trailing()}</span>{/if}
    {#if chevron}
      <span class="chevron" aria-hidden="true"><Icon name="chevron.right" size={14} weight={2.6} /></span>
    {/if}
  </span>
{/snippet}

{#if !managed}
  <!-- svelte-ignore a11y_no_static_element_interactions (a row with a click handler renders as a <button>) -->
  <svelte:element
    this={interactive ? 'button' : 'div'}
    type={interactive ? 'button' : undefined}
    class="ios-row"
    data-testid={testid}
    class:interactive
    class:selected
    aria-current={selected ? 'true' : undefined}
    {onclick}
  >
    {#if leading}
      <span class="leading">{@render leading()}</span>
    {:else if icon}
      <IconTile background={iconColor}>
        <Icon name={icon} size={18} weight={2.2} color="#fff" />
      </IconTile>
    {/if}

    <span class="content">
      <span class="text">
        <span class="title" class:destructive class:action>{title}</span>
        {#if subtitle}<span class="subtitle">{subtitle}</span>{/if}
      </span>
      {@render accessory()}
    </span>
  </svelte:element>
{:else}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions, a11y_no_noninteractive_tabindex (an option is the focus target of its listbox) -->
  <div
    class="ios-row managed"
    class:swipe={hasSwipe}
    class:option
    class:selected
    class:multi={check !== undefined}
    class:placeholder
    class:disabled
    {role}
    bind:this={rootEl}
    data-testid={testid}
    data-row-key={rowKey}
    data-row-focus={option && !placeholder ? '' : undefined}
    data-open={open === 'none' ? undefined : open}
    tabindex={option && !placeholder ? -1 : undefined}
    aria-posinset={posinset}
    aria-setsize={setsize}
    aria-selected={option ? selected : undefined}
    aria-disabled={disabled || undefined}
    aria-busy={placeholder || undefined}
    aria-label={option && label ? label : undefined}
    onclick={option && !disabled ? onclick : undefined}
    onkeydown={option ? optionKey : undefined}
  >
    {#if placeholder}
      <div class="face" aria-hidden="true">
        <span class="content">
          <span class="text"><span class="bar long"></span><span class="bar short"></span></span>
        </span>
      </div>
    {:else}
      {#if swipeLeading}
        <div class="actions leading" bind:this={leadingEl} data-no-swipe data-actions="leading">
          {@render swipeLeading()}
        </div>
      {/if}
      {#if swipeTrailing}
        <div class="actions trailing" bind:this={trailingEl} data-no-swipe data-actions="trailing">
          {@render swipeTrailing()}
        </div>
      {/if}
      <!-- svelte-ignore a11y_no_static_element_interactions (the swipe gesture is pointer-only; the actions are real buttons) -->
      <div
        class="face"
        class:dragging
        data-testid="{testid}-face"
        bind:this={faceEl}
        data-row-focus={!option && !tappable ? '' : undefined}
        tabindex={!option && !tappable ? -1 : undefined}
        style:transform={offset === 0 ? undefined : `translateX(${offset}px)`}
        onpointerdown={down}
        onpointermove={move}
        onpointerup={up}
        onpointercancel={up}
        onclickcapture={guard}
      >
        {#if check !== undefined}
          <span class="check" class:on={check} aria-hidden="true">
            {#if check}<Icon name="checkmark" size={13} weight={3} color="#fff" />{/if}
          </span>
        {/if}
        {#if leading}
          <span class="leading">{@render leading()}</span>
        {:else if icon}
          <IconTile background={iconColor}>
            <Icon name={icon} size={18} weight={2.2} color="#fff" />
          </IconTile>
        {/if}
        <span class="content">
          {@render body()}
          {@render accessory()}
        </span>
        {#if reorder}
          <button
            type="button"
            class="handle"
            data-reorder-handle
            data-testid="{testid}-handle"
            data-no-swipe
            aria-label="Reorder {title}. Use the up and down arrow keys."
          >
            <span aria-hidden="true"></span>
          </button>
        {/if}
        {#if tappable && !option}
          <button
            type="button"
            class="hit"
            data-row-focus
            data-testid="{testid}-tap"
            aria-label={name}
            aria-current={selected ? 'true' : undefined}
            {onclick}
          ></button>
        {/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .ios-row {
    appearance: none;
    border: 0;
    margin: 0;
    width: 100%;
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: var(--row-height);
    padding: 0 0 0 var(--row-inset);
    background: transparent;
    border-radius: var(--list-row-radius, 0);
    text-align: left;
    color: var(--label);
    font: var(--type-body);
    letter-spacing: var(--tracking-body);
    position: relative;
  }
  .interactive {
    cursor: pointer;
  }
  .interactive:active {
    background: var(--fill-secondary);
  }
  .selected,
  .selected:active {
    background: light-dark(var(--tint), color-mix(in oklab, var(--tint) 70%, black));
    color: #fff;
  }
  .leading {
    flex: none;
    display: flex;
    align-items: center;
  }
  .content {
    flex: 1;
    min-width: 0;
    align-self: stretch;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 11px var(--row-inset) 11px 0;
    position: relative;
  }
  /* Inset hairline separator — starts at the text, not the icon. */
  .content::after {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: var(--hairline);
    background: var(--list-separator, var(--separator));
  }
  .ios-row:not(.managed):last-child .content::after,
  .selected .content::after {
    display: none;
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .custom {
    flex: 1;
    min-width: 0;
  }
  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .title.destructive {
    color: var(--ios-red);
  }
  .title.action {
    color: var(--tint);
  }
  .subtitle {
    font: var(--type-subheadline);
    letter-spacing: var(--tracking-subheadline);
    color: var(--label-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .selected .subtitle,
  .selected .value,
  .selected .chevron {
    color: rgb(255 255 255 / 0.92);
  }
  .accessory {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    position: relative;
    z-index: 2; /* above the tap overlay: interactive trailing content stays clickable */
    pointer-events: none;
  }
  .accessory :global(button),
  .accessory :global(a),
  .accessory :global(input),
  .accessory :global([role='switch']),
  .accessory :global([role='button']),
  .accessory :global([role='checkbox']) {
    pointer-events: auto;
  }
  .value {
    color: var(--label-secondary);
  }
  .chevron {
    display: flex;
    color: var(--label-tertiary);
  }

  /* ── managed rows ── */
  .managed {
    display: block;
    padding: 0;
    background: var(--list-row-bg, transparent);
  }
  /* The separator sits just below the row, so the last row's is clipped by its section. */
  .managed .content::after {
    bottom: calc(var(--hairline) * -1);
  }
  /* Swiping slides the face over the actions: clip the sides, keep the separator below. */
  .managed.swipe {
    clip-path: inset(0 0 -1px 0);
  }
  .face {
    position: relative;
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: var(--row-height);
    padding-left: var(--row-inset);
    background: var(--list-row-bg, var(--bg-elevated));
    transition: transform var(--duration-fast) var(--ease-ios);
    touch-action: pan-y;
    z-index: 1;
  }
  .face.dragging {
    transition: none;
  }
:global(.ios-list.sidebar) .face {
    background: transparent;
  }
  .face:focus-visible,
  .managed.option:focus-visible {
    outline: 2px solid var(--tint);
    outline-offset: -2px;
  }
  .managed.option {
    cursor: pointer;
  }
  .managed.option:active:not(.selected) .face {
    background: var(--fill-secondary);
  }
  .managed.selected .face {
    background: light-dark(var(--tint), color-mix(in oklab, var(--tint) 70%, black));
    color: #fff;
  }
  /* Multiple selection marks rows with a check circle and leaves the row alone. */
  .managed.multi.selected .face {
    background: var(--list-row-bg, var(--bg-elevated));
    color: var(--label);
  }
  .managed.multi.selected .subtitle,
  .managed.multi.selected .value,
  .managed.multi.selected .chevron {
    color: var(--label-secondary);
  }
  .check {
    flex: none;
    width: 22px;
    height: 22px;
    margin-right: -2px;
    border-radius: 50%;
    border: 1.5px solid var(--label-tertiary);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .check.on {
    border-color: transparent;
    background: var(--tint-fill);
  }
  .hit {
    position: absolute;
    inset: 0;
    z-index: 1;
    appearance: none;
    border: 0;
    padding: 0;
    margin: 0;
    background: transparent;
    cursor: pointer;
    border-radius: inherit;
  }
  .hit:focus-visible {
    outline: 2px solid var(--tint);
    outline-offset: -2px;
  }
  .face:has(.hit:active) {
    background: var(--fill-secondary);
  }
  .managed.disabled {
    opacity: 0.4;
  }
  .custom :global(button),
  .custom :global(a),
  .custom :global(input),
  .custom :global(select),
  .custom :global(textarea),
  .custom :global([role='switch']),
  .custom :global([role='slider']),
  .custom :global([role='button']) {
    position: relative;
    z-index: 2;
  }

  .actions {
    position: absolute;
    top: 0;
    bottom: 0;
    display: flex;
    align-items: stretch;
    z-index: 0;
  }
  .actions.leading {
    left: 0;
  }
  .actions.trailing {
    right: 0;
    flex-direction: row-reverse; /* the first action sits at the edge, as in iPadOS */
  }

  .handle {
    flex: none;
    appearance: none;
    border: 0;
    background: transparent;
    position: relative;
    z-index: 2;
    width: 44px;
    min-height: var(--row-height);
    align-self: stretch;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: grab;
    touch-action: none;
    color: var(--label-tertiary);
  }
  .handle span,
  .handle span::before,
  .handle span::after {
    display: block;
    width: 18px;
    height: 2px;
    border-radius: 1px;
    background: currentColor;
  }
  .handle span {
    position: relative;
  }
  .handle span::before,
  .handle span::after {
    content: '';
    position: absolute;
    left: 0;
  }
  .handle span::before {
    top: -6px;
  }
  .handle span::after {
    top: 6px;
  }
  .handle:focus-visible {
    outline: 2px solid var(--tint);
    outline-offset: -4px;
    color: var(--tint);
  }

  .placeholder .bar {
    display: block;
    height: 12px;
    border-radius: 6px;
    background: var(--fill-tertiary);
    animation: shimmer 1.4s ease-in-out infinite;
  }
  .placeholder .bar.long {
    width: 62%;
  }
  .placeholder .bar.short {
    width: 36%;
    height: 10px;
    margin-top: 6px;
  }
  @keyframes shimmer {
    50% {
      opacity: 0.5;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .face {
      transition: none;
    }
    .placeholder .bar {
      animation: none;
    }
  }
</style>
