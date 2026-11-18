<!-- @molecule PermissionCard — an agent asking to run something. Atoms: Icon, Text, Button.
  Test ids: `testid` on the card, `${testid}-${option.optionId}` on each option. -->
<script lang="ts">
  import { Button, Icon, Text } from '@agentos/ui/atoms'

  interface Props {
    testid: string
    title: string
    options: { optionId: string; name: string; kind: string }[]
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
    <Text style="footnote" color="secondary">{answer}</Text>
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
    max-width: 520px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--ios-orange);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
</style>
