<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { Record as DnsRecord } from '@dns/rpc'
  import RecordRow from './RecordRow.svelte'
  import { record } from '../lib/fakeApi'

  interface Args {
    record: DnsRecord
    busy: boolean
    disabled: boolean
    onremove: () => void
  }

  const onremove = fn<() => void>()

  /** One record: a name and the address it answers with. */
  const { Story } = defineMeta({
    title: 'Molecules/RecordRow',
    component: RecordRow,
    tags: ['autodocs'],
    args: { record: record(1, 'nas.home', '192.168.1.10'), busy: false, disabled: false, onremove },
    beforeEach: () => {
      onremove.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <ul style="width:420px;margin:0;padding:0;background:var(--bg-secondary);color:var(--label);font:var(--type-body)">
    <RecordRow {...args} />
  </ul>
{/snippet}

<Story
  name="A name"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('dns-record-1-name')).toHaveTextContent('nas.home')
    await userEvent.click(canvas.getByTestId('dns-record-1-remove'))
    await expect(onremove).toHaveBeenCalledTimes(1)
  }}
/>

<!-- Every name under it answers too: said as the name and its wildcard. -->
<Story
  name="With every name under it"
  args={{ record: record(4, 'apps.home', 'fd00::20', true) }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('dns-record-4-name')).toHaveTextContent('apps.home and *.apps.home')
  }}
/>

<Story name="Being removed" args={{ busy: true }} />

<!-- A name as long as one may be is cut, and the button stays. -->
<Story name="A long name" args={{ record: record(9, `${'a-very-long-part-of-a-name.'.repeat(6)}home`, '2001:db8:85a3:8d3:1319:8a2e:370:7348') }} />
