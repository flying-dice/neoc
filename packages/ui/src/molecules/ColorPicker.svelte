<!--
  @molecule ColorPicker — a swatch grid of palette tokens (a radio group with arrow-key navigation: one tab stop),
  optionally a custom `#rrggbb` field (`custom`) and an opacity slider (`opacity`, applied to custom colours as
  `#rrggbbaa`). The chosen swatch shows a checkmark whose colour is picked for 4.5:1 on that swatch (data colours
  are content; the kit chooses the ink). Atoms: Icon, Slider.
  Test ids: `testid` on the group, `${testid}-swatch-<token>` per swatch, `${testid}-hex` and `${testid}-opacity`.
-->
<script lang="ts">
  import { describe } from '../aria'
  import Icon from '../atoms/Icon.svelte'
  import Slider from '../atoms/Slider.svelte'
  import {
    PALETTE_TOKENS,
    cssColor,
    formatColorValue,
    inkFor,
    isPaletteToken,
    normaliseHex,
    parseColorValue,
    type PaletteToken
  } from '../contrast'
  import { moveIndex } from '../keys'

  interface Props {
    testid: string
    /** A palette token, `#rrggbb` or `#rrggbbaa`. */
    value?: string
    /** Tokens offered (default: all). */
    palette?: readonly string[]
    /** Allow any `#rrggbb`. */
    custom?: boolean
    /** Show an opacity slider (custom colours). */
    opacity?: boolean
    /** Accessible name. */
    label: string
    /** Accessible description (read after the name). */
    'aria-description'?: string
    disabled?: boolean
    onchange?: (value: string) => void
  }

  let {
    testid,
    value = $bindable(''),
    palette,
    custom = false,
    opacity = false,
    label,
    'aria-description': description,
    disabled = false,
    onchange
  }: Props = $props()

  const tokens = $derived<PaletteToken[]>(
    palette ? (palette.filter(isPaletteToken) as PaletteToken[]) : [...PALETTE_TOKENS]
  )
  const current = $derived(parseColorValue(value))
  const selectedIndex = $derived(
    current?.kind === 'token' ? tokens.indexOf(current.token) : -1
  )
  const stop = $derived(selectedIndex >= 0 ? selectedIndex : 0)
  let group: HTMLElement | undefined = $state()

  function set(next: string): void {
    if (next === value) return
    value = next
    onchange?.(next)
  }

  function chooseToken(token: PaletteToken): void {
    if (disabled) return
    set(formatColorValue({ kind: 'token', token }))
  }

  // Arrow keys move over the grid (left/right by one, up/down by a row) and choose, like a radio group.
  function onkeydown(e: KeyboardEvent): void {
    if (disabled) return
    const from = Number((e.target as HTMLElement).closest<HTMLElement>('[data-index]')?.dataset.index ?? stop)
    const perRow = Math.max(1, Math.round(
      (group?.clientWidth ?? 0) / (group?.querySelector<HTMLElement>('[data-index]')?.offsetWidth || 44)
    ))
    let to: number | null = null
    if (e.key === 'ArrowDown') to = Math.min(tokens.length - 1, from + perRow)
    else if (e.key === 'ArrowUp') to = Math.max(0, from - perRow)
    else to = moveIndex(tokens.length, from, e.key, 'horizontal')
    if (to === null) return
    e.preventDefault()
    const token = tokens[to]
    if (!token) return
    chooseToken(token)
    group?.querySelector<HTMLElement>(`[data-index="${to}"]`)?.focus()
  }

  // Custom hex field: commit when it parses; show it invalid otherwise.
  let hexText = $state('')
  let hexDirty = false
  $effect(() => {
    if (!hexDirty) hexText = current?.kind === 'hex' ? current.hex : ''
  })
  const hexInvalid = $derived(hexText !== '' && normaliseHex(hexText) === null)
  function onhex(e: Event): void {
    hexDirty = true
    hexText = (e.currentTarget as HTMLInputElement).value
    const hex = normaliseHex(hexText.startsWith('#') ? hexText : `#${hexText}`)
    if (hex) set(formatColorValue({ kind: 'hex', hex, alpha: current?.kind === 'hex' ? current.alpha : 1 }))
  }
  function onhexblur(): void {
    hexDirty = false
  }

  const alphaPct = $derived(current?.kind === 'hex' ? Math.round(current.alpha * 100) : 100)
  function onalpha(next: number): void {
    if (current?.kind !== 'hex') return
    set(formatColorValue({ ...current, alpha: next / 100 }))
  }
</script>

<div class="ios-colorpicker" class:disabled role="group" aria-label={label} {...describe(description)} data-testid={testid}>
  <!-- svelte-ignore a11y_interactive_supports_focus (the radios inside are the tab stops) -->
  <div class="swatches" role="radiogroup" data-testid="{testid}-swatches" aria-label="{label}, palette" bind:this={group} {onkeydown}>
    {#each tokens as token, i (token)}
      {@const on = current?.kind === 'token' && current.token === token}
      <button
        type="button"
        role="radio"
        class="swatch"
        data-index={i}
        data-testid="{testid}-swatch-{token}"
        aria-checked={on}
        aria-label={token}
        tabindex={i === stop ? 0 : -1}
        {disabled}
        style:--swatch={cssColor(token)}
        style:--ink={inkFor(token)}
        onclick={() => chooseToken(token)}
      >
        <span class="chip" class:on>
          {#if on}<Icon name="checkmark" size={16} weight={3.2} />{/if}
        </span>
      </button>
    {/each}
  </div>
  {#if custom}
    <label class="custom">
      <span class="chip preview" style:--swatch={current?.kind === 'hex' ? cssColor(current.hex) : 'transparent'} aria-hidden="true"></span>
      <span class="name">Custom</span>
      <input
        class="hex"
        type="text"
        inputmode="text"
        autocomplete="off"
        spellcheck="false"
        maxlength="9"
        placeholder="#RRGGBB"
        data-testid="{testid}-hex"
        aria-label="{label}, custom colour as #RRGGBB"
        aria-invalid={hexInvalid ? 'true' : undefined}
        value={hexText}
        {disabled}
        oninput={onhex}
        onblur={onhexblur}
      />
    </label>
  {/if}
  {#if opacity}
    <div class="opacity">
      <span class="name" id="{testid}-opacity-label">Opacity</span>
      <Slider
        testid="{testid}-opacity"
        value={alphaPct}
        min={0}
        max={100}
        step={1}
        label="{label}, opacity"
        valuetext="{alphaPct} percent"
        disabled={disabled || current?.kind !== 'hex'}
        oninput={onalpha}
      />
      <span class="pct" aria-hidden="true">{alphaPct}%</span>
    </div>
  {/if}
</div>

<style>
  .ios-colorpicker {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-width: 0;
    max-width: 100%;
  }
  .disabled {
    opacity: 0.4;
  }
  .swatches {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(44px, 1fr));
    max-width: 100%;
  }
  .swatch {
    appearance: none;
    border: 0;
    background: transparent;
    display: grid;
    place-items: center;
    min-height: 44px;
    padding: 0;
    cursor: pointer;
  }
  .swatch:focus-visible {
    outline-offset: -4px;
    border-radius: 50%;
  }
  .chip {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    background: var(--swatch);
    color: var(--ink);
    box-shadow: inset 0 0 0 var(--hairline) var(--separator);
  }
  .chip.on {
    box-shadow:
      0 0 0 2px var(--bg-grouped-elevated),
      0 0 0 4px var(--label);
  }
  .custom,
  .opacity {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 44px;
  }
  .preview {
    flex: none;
    box-shadow: inset 0 0 0 1px var(--separator);
  }
  .name {
    font: var(--type-body);
    letter-spacing: var(--tracking-body);
  }
  .hex {
    flex: 1;
    min-width: 0;
    min-height: 36px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-sm);
    background: var(--fill-tertiary);
    font: var(--type-body);
    font-family: var(--font-mono);
    letter-spacing: 0;
    color: var(--label);
    text-transform: lowercase;
  }
  .hex[aria-invalid='true'] {
    outline: 2px solid var(--ios-red);
  }
  .pct {
    min-width: 40px;
    text-align: end;
    font: var(--type-subheadline);
    font-variant-numeric: tabular-nums;
    color: var(--label-secondary);
  }
</style>
