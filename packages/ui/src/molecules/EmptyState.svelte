<!--
  @molecule EmptyState — what a screen shows when there is nothing to show (`app-design` styleguide, section 8): a muted glyph, a
  title, an optional message and one call to action in `action`. Atoms: Icon. Optional test id: `testid`.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import Icon from '../atoms/Icon.svelte'
  import type { IconName } from '../icons'

  interface Props {
    testid?: string
    title: string
    message?: string
    icon?: IconName
    style?: string
    action?: Snippet
  }

  let { testid, title, message, icon, style, action }: Props = $props()
</script>

<section class="ios-empty" data-testid={testid} aria-label={title} {style}>
  {#if icon}<span class="glyph" aria-hidden="true"><Icon name={icon} size={44} weight={1.6} /></span>{/if}
  <h2 class="title">{title}</h2>
  {#if message}<p class="message">{message}</p>{/if}
  {#if action}<div class="action">{@render action()}</div>{/if}
</section>

<style>
  .ios-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 32px var(--row-inset);
    text-align: center;
    color: var(--label-secondary);
    min-width: 0;
  }
  .glyph {
    display: flex;
    color: var(--label-tertiary);
    margin-bottom: 6px;
  }
  .title {
    margin: 0;
    font: var(--type-title3);
    font-weight: 600;
    letter-spacing: var(--tracking-title3);
    color: var(--label);
    overflow-wrap: anywhere;
  }
  .message {
    margin: 0;
    max-width: 32ch;
    font: var(--type-subheadline);
    letter-spacing: var(--tracking-subheadline);
    overflow-wrap: anywhere;
  }
  .action {
    margin-top: 12px;
  }
</style>
