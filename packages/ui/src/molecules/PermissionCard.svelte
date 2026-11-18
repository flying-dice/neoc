<!-- @molecule PermissionCard — an agent asking to run something: allow once, always, or deny. Atoms: Icon, Text, Button.
  Test ids: `testid` on the card, `${testid}-${option.optionId}` on each option. -->
<script lang="ts">
  import Button from '../atoms/Button.svelte'
  import Icon from '../atoms/Icon.svelte'
  import Text from '../atoms/Text.svelte'
  import type { ChatPermissionOption } from '../types'

  interface Props {
    testid: string
    title: string
    options: ChatPermissionOption[]
    /** What was chosen once the request is answered. */
    answer?: string
    onchoose: (optionId: string | null) => void
  }

  let { testid, title, options, answer, onchoose }: Props = $props()
</script>

<div class="permission" role="group" aria-label="Permission request" data-testid={testid}>
  <div class="head">
    <Icon name="hand.raised" size={18} />
    <Text style="subheadline" weight="semibold">Allow {title}?</Text>
  </div>
  {#if answer}
    <span role="status"><Text style="footnote" color="secondary">{answer}</Text></span>
  {:else}
    <div class="actions">
      {#each options as o (o.optionId)}
        <Button
          testid="{testid}-{o.optionId}"
          size="small"
          capsule
          variant={o.kind.startsWith('allow')
            ? o.kind === 'allow_once'
              ? 'filled'
              : 'tinted'
            : 'gray'}
          destructive={o.kind.startsWith('reject')}
          onclick={() => onchoose(o.optionId)}
        >
          {o.name}
        </Button>
      {/each}
      {#if options.length === 0}
        <Button
          testid="{testid}-deny"
          size="small"
          capsule
          variant="gray"
          destructive
          onclick={() => onchoose(null)}
        >
          Deny
        </Button>
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
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
</style>
