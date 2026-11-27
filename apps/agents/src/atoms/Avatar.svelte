<!-- @atom Avatar — an agent as a circle: its picture, or its initials on a tint of its own; named by the agent's name -->
<script lang="ts">
  import { initialsOf, tintOf } from '../lib/avatar'

  interface Props {
    /** The agent's id: it picks the tint behind the initials. */
    id: string
    /** The agent's name: the initials, and what a screen reader says. */
    name: string
    /** Its picture, a `data:` URL; empty for none. */
    picture?: string
    /** Pixels across. */
    size?: number
  }

  let { id, name, picture = '', size = 32 }: Props = $props()
</script>

{#if picture !== ''}
  <img class="avatar" src={picture} alt={name} width={size} height={size} />
{:else}
  <!-- the initials are drawn, not written: the circle is a picture named by the agent's name, with no text of its own
       to be read twice in a row that names the agent already -->
  <span
    class="avatar initials"
    role="img"
    aria-label={name}
    data-initials={initialsOf(name)}
    style:--size="{size}px"
    style:--fill="var(--ios-{tintOf(id)})"
  ></span>
{/if}

<style>
  .avatar {
    flex: none;
    display: block;
    box-sizing: border-box;
    border-radius: 50%;
    object-fit: cover;
  }
  .initials {
    display: grid;
    place-items: center;
    width: var(--size);
    height: var(--size);
    background: var(--fill);
    /* black or white, whichever reads on the tint (the kit's own rule for text on a palette colour) */
    color: oklch(from var(--fill) clamp(0, (0.59 - l) * 1000, 1) 0 0);
    font-size: calc(var(--size) * 0.4);
    font-weight: 600;
    line-height: 1;
    letter-spacing: 0;
    user-select: none;
  }
  .initials::before {
    content: attr(data-initials);
  }
</style>
