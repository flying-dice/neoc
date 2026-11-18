<!--
  @molecule OptionList — "one of many" as a grouped list with a checkmark on the chosen row (the inline Picker).
  A listbox: one tab stop, Up/Down/Home/End move, Enter or Space chooses, type to jump to a label. `multiple` lets
  several rows be checked. Atoms: Icon.
  Test ids: `testid` on the list, `${testid}-option-<key>` on each row.
-->
<script lang="ts">
  import { describe } from '../aria'
  import Icon from '../atoms/Icon.svelte'
  import type { OptionItem } from '../types'
  import { isActivateKey, isPrintable, moveIndex, nextEnabled, typeAhead } from '../keys'

  interface Props {
    testid: string
    options: OptionItem[]
    /** Keys of the checked rows. */
    selected?: string[]
    multiple?: boolean
    /** Accessible name. */
    label: string
    /** Accessible description (read after the name). */
    'aria-description'?: string
    disabled?: boolean
    onselect?: (key: string) => void
  }

  let { testid, options, selected = [], multiple = false, label, 'aria-description': description, disabled = false, onselect }: Props = $props()

  let list: HTMLElement | undefined = $state()
  let buffer = ''
  let bufferTimer: ReturnType<typeof setTimeout> | undefined
  const firstSelected = $derived(options.findIndex((o) => selected.includes(o.key) && !o.disabled))
  const stop = $derived(firstSelected >= 0 ? firstSelected : Math.max(0, options.findIndex((o) => !o.disabled)))
  const slug = (key: string): string => key.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')

  function focusIndex(i: number): void {
    list?.querySelector<HTMLElement>(`[data-index="${i}"]`)?.focus()
  }

  function choose(i: number): void {
    const option = options[i]
    if (option && !option.disabled && !disabled) onselect?.(option.key)
  }

  function onkeydown(e: KeyboardEvent): void {
    if (disabled) return
    const current = Number((e.target as HTMLElement).closest<HTMLElement>('[role="option"]')?.dataset.index ?? stop)
    const flags = options.map((o) => o.disabled === true)
    const to = moveIndex(options.length, current, e.key, 'vertical')
    if (to !== null) {
      e.preventDefault()
      const dir = e.key === 'ArrowUp' ? -1 : 1
      const next = e.key === 'Home' ? nextEnabled(flags, -1, 1) : e.key === 'End' ? nextEnabled(flags, 0, -1) : nextEnabled(flags, current, dir)
      focusIndex(next)
      return
    }
    if (isActivateKey(e.key)) {
      e.preventDefault()
      choose(current)
      return
    }
    if (isPrintable(e)) {
      buffer += e.key
      clearTimeout(bufferTimer)
      bufferTimer = setTimeout(() => (buffer = ''), 600)
      const hit = typeAhead(options.map((o) => o.label), buffer, buffer.length === 1 ? current : current - 1)
      if (hit >= 0) focusIndex(hit)
    }
  }
</script>

<div
  class="ios-options"
  role="listbox"
  tabindex="-1"
  aria-label={label}
  {...describe(description)}
  aria-multiselectable={multiple || undefined}
  aria-disabled={disabled || undefined}
  data-testid={testid}
  bind:this={list}
  {onkeydown}
>
  {#each options as option, i (option.key)}
    {@const on = selected.includes(option.key)}
    <button
      type="button"
      role="option"
      class="row"
      class:on
      tabindex={i === stop ? 0 : -1}
      data-index={i}
      data-testid="{testid}-option-{slug(option.key)}"
      aria-selected={on}
      disabled={disabled || option.disabled}
      onclick={() => choose(i)}
    >
      {#if option.icon}<span class="glyph" aria-hidden="true"><Icon name={option.icon} size={20} weight={2} /></span>{/if}
      <span class="title">{option.label}</span>
      <span class="check" aria-hidden="true">{#if on}<Icon name="checkmark" size={18} weight={2.6} />{/if}</span>
    </button>
  {/each}
</div>

<style>
  .ios-options {
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border-radius: var(--radius-md);
    background: var(--bg-grouped-elevated-secondary);
    outline: 0;
  }
  .row {
    appearance: none;
    border: 0;
    margin: 0;
    background: transparent;
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: var(--row-height);
    padding: 0 var(--row-inset);
    font: var(--type-body);
    letter-spacing: var(--tracking-body);
    color: var(--label);
    text-align: start;
    cursor: pointer;
    position: relative;
  }
  .row + .row::before {
    content: '';
    position: absolute;
    top: 0;
    inset-inline: var(--row-inset) 0;
    border-top: var(--hairline) solid var(--separator);
  }
  .row:active:not(:disabled) {
    background: var(--fill-secondary);
  }
  .row:disabled {
    color: var(--label-tertiary);
    cursor: default;
  }
  .row:focus-visible {
    outline-offset: -3px;
  }
  .glyph {
    display: grid;
    color: var(--tint);
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .check {
    display: grid;
    width: 20px;
    color: var(--tint);
  }
</style>
