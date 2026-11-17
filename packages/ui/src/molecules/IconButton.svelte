<!-- @molecule IconButton — Button with an SF-style glyph and optional title. Atoms: Button, Icon. Test id: `testid`. -->
<script lang="ts">
  import Button from '../atoms/Button.svelte'
  import Icon from '../atoms/Icon.svelte'
  import type { IconName } from '../icons'

  interface Props {
    testid: string
    icon: IconName
    /** Visible title; omit for a square glyph-only button (then `label` is required for a11y). */
    title?: string
    label?: string
    variant?: 'filled' | 'tinted' | 'gray' | 'plain'
    size?: 'small' | 'medium' | 'large'
    capsule?: boolean
    tint?: string
    destructive?: boolean
    disabled?: boolean
    block?: boolean
    onclick?: () => void
  }

  let {
    testid,
    icon,
    title,
    label,
    variant = 'plain',
    size = 'medium',
    capsule = false,
    tint,
    destructive = false,
    disabled = false,
    block = false,
    onclick
  }: Props = $props()

  const glyph = $derived(size === 'small' ? 15 : size === 'large' ? 22 : 19)
</script>

<Button
  {testid}
  {variant}
  {size}
  {capsule}
  {tint}
  {destructive}
  {disabled}
  {block}
  square={!title}
  aria-label={title ? undefined : label}
  {onclick}
>
  <Icon name={icon} size={glyph} weight={2.2} />
  {#if title}{title}{/if}
</Button>
