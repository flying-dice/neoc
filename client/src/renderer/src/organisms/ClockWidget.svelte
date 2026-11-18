<!-- @organism ClockWidget — home-screen date & time widget. Atoms: Card, Text. Test id: `testid`. -->
<script lang="ts">
  import { Card, Text } from '@agentos/ui/atoms'

  interface Props {
    testid: string
  }

  let { testid }: Props = $props()

  let now = $state(new Date())
  $effect(() => {
    const t = setInterval(() => (now = new Date()), 1000 * 15)
    return () => clearInterval(t)
  })

  const weekday = $derived(now.toLocaleDateString([], { weekday: 'long' }).toUpperCase())
  const day = $derived(now.getDate())
  const time = $derived(now.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' }))
</script>

<Card {testid} size="small" surface="material">
  <div class="clock">
    <Text style="caption1" weight="semibold" color="destructive">{weekday}</Text>
    <span class="day">{day}</span>
    <Text style="subheadline" color="secondary">{time}</Text>
  </div>
</Card>

<style>
  .clock {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  .day {
    font: 300 64px/1 var(--font-display);
    letter-spacing: -1px;
    margin: 4px 0 auto;
  }
</style>
