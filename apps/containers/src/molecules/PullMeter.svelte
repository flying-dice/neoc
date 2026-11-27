<!-- @molecule PullMeter — how the pull of an image goes: its name, how much of it is downloaded of how much (or Docker's word for the step it is at), and a meter -->
<script lang="ts">
  type Pulling = VosApp.Containers.PullingParams
  import { bytes } from '../lib/format'

  interface Props {
    pulling: Pulling
    testid: string
  }

  let { pulling, testid }: Props = $props()

  const share = $derived(pulling.total > 0 ? Math.min(1, pulling.current / pulling.total) : undefined)
</script>

<div class="pull" data-testid={testid}>
  <p class="pulling">
    <span>Pulling <span class="mono">{pulling.image}</span></span>
    <span class="amount">{pulling.total > 0 ? `${bytes(pulling.current)} of ${bytes(pulling.total)}` : pulling.status}</span>
  </p>
  <ui-meter value={share === undefined ? undefined : share * 100} label="Pulling {pulling.image}"></ui-meter>
</div>

<style>
  .pull {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 10px 16px;
    border-radius: var(--radius-lg);
    background: var(--bg);
  }
  .pulling {
    margin: 0;
    display: flex;
    justify-content: space-between;
    gap: 12px;
    font: var(--type-subheadline);
  }
  .pulling > :first-child {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .amount {
    color: var(--label-secondary);
    white-space: nowrap;
  }
  .mono {
    font-family: var(--font-mono);
  }
</style>
