<!-- @molecule LogRow — one log: what it says, coloured by how severe it is, with when and where it was written and by which app -->
<script lang="ts">
  import { SEVERITY_LABEL, SOURCE_LABEL, clock, type Log } from '../lib/telemetry'

  interface Props {
    log: Log
    /** Show the app it came from (the Logs view mixes every app's; a trace is one app's). */
    showApp?: boolean
    selected?: boolean
    onpress: () => void
    testid: string
  }

  let { log, showApp = false, selected = false, onpress, testid }: Props = $props()

  const sub = $derived([clock(log.timeMs), SOURCE_LABEL[log.source], showApp ? log.app : undefined].filter(Boolean).join(' · '))
  const icon = $derived(log.severity === 'error' || log.severity === 'warn' ? 'exclamationmark.triangle' : 'info.circle')
  const color = $derived(log.severity === 'error' ? 'red' : log.severity === 'warn' ? 'orange' : 'gray')
</script>

<ui-row
  heading={log.body || '(no message)'}
  subheading={sub}
  {icon}
  icon-color={color}
  label="{log.body || '(no message)'} {sub}, {SEVERITY_LABEL[log.severity]}"
  {selected}
  {onpress}
  data-testid={testid}
></ui-row>
