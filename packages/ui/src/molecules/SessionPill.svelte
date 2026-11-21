<!--
  @molecule SessionPill — a quiet capsule showing a session setting's current
  value; clicking opens the native menu (a real <select> sits on top), so it
  is keyboard- and screen-reader-friendly for free. Atoms: Icon.
  Test ids: `testid` on the select, `${testid}-value` on the visible value.
-->
<script lang="ts" module>
  export interface PillOption {
    value: string
    name: string
    description?: string | null
    group?: string
  }
</script>

<script lang="ts">
  import Icon from '../atoms/Icon.svelte'
  import type { IconName } from '../icons'

  interface Props {
    testid: string
    /** Accessible name, e.g. "Model". */
    label: string
    icon?: IconName
    value: string
    options: PillOption[]
    /** Text shown when `value` matches no option (e.g. a placeholder). */
    placeholder?: string
    disabled?: boolean
    onchange: (value: string) => void
  }

  let {
    testid,
    label,
    icon,
    value,
    options,
    placeholder,
    disabled = false,
    onchange
  }: Props = $props()

  const current = $derived(options.find((o) => o.value === value)?.name ?? placeholder ?? label)
  /** Options bucketed by group, in first-seen order ("" = ungrouped). */
  const groups = $derived(
    options.reduce<[string, PillOption[]][]>((acc, o) => {
      const key = o.group ?? ''
      const bucket = acc.find(([g]) => g === key)
      if (bucket) bucket[1].push(o)
      else acc.push([key, [o]])
      return acc
    }, [])
  )
</script>

<label class="pill" class:disabled title={label}>
  {#if icon}<Icon name={icon} size={13} weight={2.2} />{/if}
  <span class="value" data-testid="{testid}-value">{current}</span>
  <Icon name="chevron.down" size={10} weight={2.6} />
  <select
    data-testid={testid}
    aria-label={label}
    {disabled}
    {value}
    onchange={(e) => onchange((e.currentTarget as HTMLSelectElement).value)}
  >
    {#if placeholder && !options.some((o) => o.value === value)}
      <option {value} disabled>{placeholder}</option>
    {/if}
    {#each groups as [group, items] (group)}
      {#if group}
        <optgroup label={group}>
          {#each items as o (o.value)}<option value={o.value} title={o.description ?? undefined}
              >{o.name}</option
            >{/each}
        </optgroup>
      {:else}
        {#each items as o (o.value)}<option value={o.value} title={o.description ?? undefined}
            >{o.name}</option
          >{/each}
      {/if}
    {/each}
  </select>
</label>

<style>
  .pill {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 24px;
    padding: 0 8px;
    border-radius: 999px;
    color: var(--label-secondary);
    font: var(--type-caption1);
    font-weight: 500;
    white-space: nowrap;
    cursor: pointer;
    transition: background-color var(--duration-fast) ease;
  }
  .pill:hover,
  .pill:focus-within {
    background: var(--fill-tertiary);
    color: var(--label);
  }
  .pill:focus-within {
    outline: 3px solid var(--focus-ring);
    outline-offset: 1px;
  }
  .disabled {
    opacity: 0.45;
    pointer-events: none;
  }
  .value {
    max-width: 160px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  select {
    position: absolute;
    inset: 0;
    width: 100%;
    opacity: 0;
    cursor: pointer;
    font: inherit;
  }
</style>
