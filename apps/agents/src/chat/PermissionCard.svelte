<!-- PermissionCard — an agent asking to run something: allow once, always, or deny. Kit: ui-icon, ui-button.
  Test ids: `testid` on the card, `${testid}-${option.optionId}` on each option. -->
<script lang="ts">
  import { uniqueBy } from '../agui/unique'
  import type { ChatPermissionOption } from './types'

  interface Props {
    testid: string
    title: string
    options: ChatPermissionOption[]
    /** What was chosen once the request is answered. */
    answer?: string
    onchoose: (optionId: string | null) => void
  }

  let { testid, title, options, answer, onchoose }: Props = $props()

  // The options come from an agent, and each is keyed by its id: a repeated id must not crash the list.
  const shown = $derived(uniqueBy((o: ChatPermissionOption) => o.optionId)(options))
</script>

<div class="permission" role="group" aria-label="Permission request" data-testid={testid}>
  <div class="head">
    <ui-icon name="hand.raised" size={18}></ui-icon>
    <span class="title">Allow {title}?</span>
  </div>
  {#if answer}
    <span class="answer" role="status">{answer}</span>
  {:else}
    <div class="actions">
      {#each shown as o (o.optionId)}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions (a kit button handles its own keys) -->
        <ui-button
          data-testid="{testid}-{o.optionId}"
          size="small"
          shape="capsule"
          hit
          variant={o.kind.startsWith('allow')
            ? o.kind === 'allow_once'
              ? 'filled'
              : 'tinted'
            : 'gray'}
          destructive={o.kind.startsWith('reject')}
          onclick={() => onchoose(o.optionId)}
        >
          {o.name}
        </ui-button>
      {/each}
      {#if shown.length === 0}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions (a kit button handles its own keys) -->
        <ui-button
          data-testid="{testid}-deny"
          size="small"
          shape="capsule"
          hit
          variant="gray"
          destructive
          onclick={() => onchoose(null)}
        >
          Deny
        </ui-button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .permission {
    margin: 6px 16px;
    padding: 12px 14px;
    border-radius: 14px;
    background: color-mix(in srgb, var(--ios-orange) 12%, var(--bg-elevated));
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--ios-orange) 35%, transparent);
    display: flex;
    flex-direction: column;
    gap: 10px;
    max-width: min(520px, calc(100% - 32px));
    overflow-wrap: anywhere;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--ios-orange);
    min-width: 0;
  }
  .title {
    font: var(--type-subheadline);
    font-weight: 600;
    color: var(--label);
  }
  .answer {
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
</style>
