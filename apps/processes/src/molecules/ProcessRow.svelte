<!-- @molecule ProcessRow — one process in the list: its name, its PID and user, and its CPU and memory in columns that line up with the rows around it -->
<script lang="ts">
  import type { ProcessRow } from '@processes/rpc'
  import { bytes, percent } from '../lib/format'

  interface Props {
    process: ProcessRow
    /** It is the process that is shown on the right. */
    selected?: boolean
    onpress: () => void
  }

  let { process, selected = false, onpress }: Props = $props()

  const sub = $derived(process.user ? `PID ${process.pid} · ${process.user}` : `PID ${process.pid}`)
  const cpu = $derived(percent(process.cpu))
  const memory = $derived(bytes(process.memory))
</script>

<ui-row
  heading={process.name}
  subheading={sub}
  label="{process.name} {sub} {cpu} {memory}"
  {selected}
  {onpress}
  data-testid="processes-row-{process.pid}"
>
  <ui-value-pair slot="trailing" main={cpu} secondary={memory}></ui-value-pair>
</ui-row>
