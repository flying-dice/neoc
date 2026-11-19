<!--
  @molecule DisclosureGroup — content that expands and collapses under a label (UIKit's disclosure row). Atoms:
  Icon. Test ids: `testid` on the header button, `${testid}-content` on the region. `aria-expanded` and
  `aria-controls` tie them together; Enter and Space toggle (it is a native button).
-->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import Icon from '../atoms/Icon.svelte'

  interface Props {
    testid: string
    label: string
    expanded?: boolean
    disabled?: boolean
    onchange?: (expanded: boolean) => void
    /** Custom label content (otherwise the `label` text). */
    labelContent?: Snippet
    children: Snippet
  }

  let { testid, label, expanded = $bindable(false), disabled = false, onchange, labelContent, children }: Props = $props()
  const uid = $props.id()

  function toggle(): void {
    expanded = !expanded
    onchange?.(expanded)
  }
</script>

<div class="ios-disclosure" role="group" aria-label={label}>
  <button
    type="button"
    class="head"
    data-testid={testid}
    aria-expanded={expanded}
    aria-controls="{uid}-content"
    {disabled}
    onclick={toggle}
  >
    <span class="chevron" class:open={expanded} aria-hidden="true">
      <Icon name="chevron.right" size={14} weight={2.6} />
    </span>
    <span class="label">{#if labelContent}{@render labelContent()}{:else}{label}{/if}</span>
  </button>
  <div class="content" id="{uid}-content" data-testid="{testid}-content" role="region" aria-label={label} hidden={!expanded}>
    {@render children()}
  </div>
</div>

<style>
  .ios-disclosure {
    min-width: 0;
  }
  .head {
    appearance: none;
    border: 0;
    background: transparent;
    width: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: var(--row-height);
    padding: 0 var(--row-inset);
    text-align: left;
    color: var(--label);
    font: var(--type-body);
    letter-spacing: var(--tracking-body);
    cursor: pointer;
  }
  .head:active {
    background: var(--fill-secondary);
  }
  .head:focus-visible {
    outline: 3px solid var(--focus-ring);
    outline-offset: -2px;
  }
  .head:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .chevron {
    flex: none;
    display: flex;
    color: var(--tint);
    transition: transform var(--duration-fast) var(--ease-ios);
  }
  .chevron.open {
    transform: rotate(90deg);
  }
  .label {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .content {
    padding-left: calc(var(--row-inset) + 22px);
  }
  .content[hidden] {
    display: none;
  }
  @media (prefers-reduced-motion: reduce) {
    .chevron {
      transition: none;
    }
  }
</style>
