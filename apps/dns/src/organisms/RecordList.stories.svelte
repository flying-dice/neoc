<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { Record as DnsRecord } from 'app:rpc'
  import RecordList from './RecordList.svelte'
  import type { Problems, RecordDraft } from '../lib/dns.svelte'
  import { RECORDS } from '../lib/fakeApi'

  interface Args {
    records: readonly DnsRecord[]
    draft: RecordDraft
    problems: Problems<'name' | 'address'>
    working: boolean
    removing: (record: DnsRecord) => boolean
    ondraft: (change: Partial<RecordDraft>) => void
    onadd: () => void
    onremove: (record: DnsRecord) => void
  }

  const onremove = fn<(record: DnsRecord) => void>()

  /** The names the server answers for, with the form that adds one. */
  const { Story } = defineMeta({
    title: 'Organisms/RecordList',
    component: RecordList,
    tags: ['autodocs'],
    args: {
      records: RECORDS,
      draft: { name: '', address: '', subdomains: false },
      problems: { name: null, address: null },
      working: false,
      removing: () => false,
      ondraft: fn(),
      onadd: fn(),
      onremove
    },
    argTypes: { removing: { control: false } },
    beforeEach: () => {
      onremove.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:560px;height:420px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <RecordList {...args} />
  </div>
{/snippet}

<Story
  name="A home network"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('dns-records-summary')).toHaveTextContent('4 names')
    await userEvent.click(canvas.getByTestId('dns-record-3-remove'))
    await expect(onremove).toHaveBeenCalledWith(RECORDS[2])
  }}
/>

<Story
  name="No names"
  args={{ records: [] }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('dns-empty')).toBeVisible()
    await expect(canvas.getByTestId('dns-records-summary')).toHaveTextContent('No names')
  }}
/>

<Story name="One being removed" args={{ working: true, removing: (record: DnsRecord) => record.id === 3 }} />
