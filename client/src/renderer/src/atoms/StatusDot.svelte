<!-- @atom StatusDot — small coloured dot for agent state. -->
<script lang="ts">
  interface Props {
    status: 'idle' | 'starting' | 'running' | 'waiting' | 'error'
  }

  let { status }: Props = $props()
  const color = $derived(
    status === 'running' || status === 'starting'
      ? 'var(--ios-green)'
      : status === 'waiting'
        ? 'var(--ios-orange)'
        : status === 'error'
          ? 'var(--ios-red)'
          : 'var(--ios-gray3)'
  )
</script>

<span
  class="dot"
  class:pulse={status === 'running' || status === 'starting'}
  style:background={color}
  aria-label={status}
></span>

<style>
  .dot {
    display: inline-block;
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
  .pulse {
    animation: pulse 1.6s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.4;
    }
  }
</style>
