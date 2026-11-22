<!-- @organism RecordList — the names the server answers for: the form that adds one, and each record with the button that takes it away -->
<script lang="ts">
  import type { Record as DnsRecord } from 'app:rpc'
  import RecordForm from '../molecules/RecordForm.svelte'
  import RecordRow from '../molecules/RecordRow.svelte'
  import type { Problems, RecordDraft } from '../lib/dns.svelte'
  import { recordsSummary } from '../lib/format'

  interface Props {
    records: readonly DnsRecord[]
    /** The record that is being typed. */
    draft: RecordDraft
    /** What is wrong with each field of it as it is typed. */
    problems: Problems<'name' | 'address'>
    /** The backend's word on the record that was sent and not added. */
    refused?: string | null
    /** A change is in flight: the server is being made again. */
    working?: boolean
    /** Whether a call is in flight for a record. */
    removing: (record: DnsRecord) => boolean
    ondraft: (change: Partial<RecordDraft>) => void
    onadd: () => void
    onremove: (record: DnsRecord) => void
  }

  let { records, draft, problems, refused = null, working = false, removing, ondraft, onadd, onremove }: Props = $props()
</script>

<section class="records" aria-label="Names">
  <RecordForm {draft} {problems} {refused} busy={working} {ondraft} {onadd} />
  <p class="summary" data-testid="dns-records-summary">{recordsSummary(records.length)}</p>
  <div class="list-box">
    {#if records.length > 0}
      <ul class="list" data-testid="dns-records">
        {#each records as record (record.id)}
          <RecordRow {record} busy={removing(record)} disabled={working} onremove={() => onremove(record)} />
        {/each}
      </ul>
    {:else}
      <ui-empty-state
        icon="network"
        heading="No Names"
        message="Give a machine of your network a name above, like nas.home, and the server answers with its address."
        data-testid="dns-empty"
      ></ui-empty-state>
    {/if}
  </div>
</section>

<style>
  /* the form and the names scroll as one: in a short window the form is most of what fits */
  .records {
    box-sizing: border-box;
    height: 100%;
    overflow-y: auto;
    padding-top: 4px;
  }
  .summary {
    margin: 0;
    padding: 8px 16px 4px;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  .list-box {
    padding: 0 16px 16px;
  }
  .list {
    margin: 0;
    padding: 0;
    border-radius: var(--radius-lg);
    background: var(--bg-secondary);
  }
</style>
