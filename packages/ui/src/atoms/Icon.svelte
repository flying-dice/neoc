<!-- @atom Icon — SF Symbols–style glyph from the icon set. -->
<script lang="ts">
  import { icons, type IconName } from '../icons'

  interface Props {
    name: IconName
    size?: number
    /** Stroke weight — think SF Symbols weights: 1.5 light, 2 regular, 2.5 semibold. */
    weight?: number
    color?: string
    label?: string
  }

  let { name, size = 22, weight = 2, color = 'currentColor', label }: Props = $props()
</script>

<svg
  class="ios-icon"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke={color}
  stroke-width={weight}
  stroke-linecap="round"
  stroke-linejoin="round"
  role={label ? 'img' : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : 'true'}
>
  {@html icons[name]}
</svg>

<style>
  .ios-icon {
    display: inline-block;
    flex: none;
    vertical-align: middle;
    color: inherit;
  }
</style>
