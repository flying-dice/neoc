<!--
  @atom Avatar — a person or agent: an image, or initials on a data colour (a palette token or `#rrggbb`; the
  initials' colour is chosen for 4.5:1 on it). With no `tint` the colour comes from the name, stably. A status dot
  differs in shape as well as colour. The accessible name is the name plus the status. Not interactive.
-->
<script lang="ts">
  import { AVATAR_SIZES, colorForName, initials, safeImageSrc, type AvatarSize, type AvatarStatus } from '../avatar'
  import { inkFor } from '../contrast'
  import { cssColor } from '../contrast'

  interface Props {
    /** Used for the initials, the automatic colour and the accessible name. */
    name?: string
    /** An https URL or same-origin path; anything else is ignored. Falls back to the initials if it fails. */
    image?: string
    size?: AvatarSize
    status?: AvatarStatus
    /** A data colour: palette token or `#rrggbb`. */
    tint?: string
    /** Accessible name when `name` is not enough (the common `label`). */
    label?: string
    testid?: string
  }

  let { name = '', image, size = 'md', status, tint, label, testid }: Props = $props()

  const px = $derived(AVATAR_SIZES[size])
  const text = $derived(initials(name))
  const fill = $derived(cssColor(tint) ?? `var(--ios-${colorForName(name || 'agent')})`)
  const ink = $derived(inkFor(tint) ?? inkFor(colorForName(name || 'agent')))
  const src = $derived(image ? safeImageSrc(image) : null)
  let failed = $state(false)
  $effect(() => {
    void src
    failed = false
  })
  const accessibleName = $derived(
    [label ?? (name || 'Avatar'), status ? `(${status})` : ''].filter(Boolean).join(' ')
  )
</script>

<span
  class="ios-avatar"
  role="img"
  aria-label={accessibleName}
  data-testid={testid}
  style:--size="{px}px"
  style:--avatar-fill={fill}
  style:--ink={ink}
>
  {#if src && !failed}
    <img {src} alt="" loading="lazy" referrerpolicy="no-referrer" onerror={() => (failed = true)} />
  {:else if text}
    <span class="initials" aria-hidden="true">{text}</span>
  {:else}
    <svg viewBox="0 0 24 24" width="55%" height="55%" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
      <circle cx="12" cy="8" r="4" /><path d="M4.5 20c.8-3.7 3.9-6 7.5-6s6.7 2.3 7.5 6" />
    </svg>
  {/if}
  {#if status}
    <span class="status {status}" aria-hidden="true"></span>
  {/if}
</span>

<style>
  .ios-avatar {
    position: relative;
    display: inline-grid;
    place-items: center;
    flex: none;
    width: var(--size);
    height: var(--size);
    border-radius: 50%;
    background: var(--avatar-fill);
    color: var(--ink);
    font-weight: 600;
    font-size: calc(var(--size) * 0.4);
    line-height: 1;
    font-family: var(--font-text);
  }
  img {
    width: 100%;
    height: 100%;
    border-radius: 50%;
    object-fit: cover;
  }
  .initials {
    letter-spacing: 0;
    user-select: none;
  }
  .status {
    position: absolute;
    right: -1px;
    bottom: -1px;
    width: max(8px, calc(var(--size) * 0.28));
    height: max(8px, calc(var(--size) * 0.28));
    border-radius: 50%;
    box-shadow: 0 0 0 2px var(--bg-elevated);
    background: var(--ios-green);
  }
  /* Busy: a bar across it. Away: a crescent notch. Offline: hollow. Shape carries the state, not only colour. */
  .busy {
    background: linear-gradient(var(--bg-elevated), var(--bg-elevated)) center / 55% 2px no-repeat, var(--ios-red);
  }
  .away {
    background: radial-gradient(circle at 70% 30%, var(--bg-elevated) 0 28%, transparent 30%), var(--ios-orange);
  }
  .offline {
    background: var(--bg-elevated);
    box-shadow:
      0 0 0 2px var(--bg-elevated),
      inset 0 0 0 2px var(--label-secondary);
  }
</style>
