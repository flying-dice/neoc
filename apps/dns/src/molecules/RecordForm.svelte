<!-- @molecule RecordForm — the next record as it is typed, as a form of iPadOS is: a row each for the name, the address and whether every name under it answers too; what is wrong with them in the footer under the rows; and an Add button that cannot be pressed until there is something right to add -->
<script lang="ts">
  import type { Problems, RecordDraft } from '../lib/dns.svelte'

  interface Props {
    /** What is typed so far. */
    draft: RecordDraft
    /** What is wrong with each field as it is typed. */
    problems: Problems<'name' | 'address'>
    /** The backend's word on the record that was sent and not added (the name has that address already). */
    refused?: string | null
    /** A call is in flight: nothing more is added until it is answered. */
    busy?: boolean
    ondraft: (change: Partial<RecordDraft>) => void
    onadd: () => void
  }

  let { draft, problems, refused = null, busy = false, ondraft, onadd }: Props = $props()

  const valueOf = (e: Event): string => (e.currentTarget as unknown as { value: string }).value
  // there is something to add, and nothing wrong with it: only then can Add be pressed
  const ready = $derived(draft.name.trim() !== '' && draft.address.trim() !== '' && problems.name === null && problems.address === null)

  // A field says what is wrong with it once the person has left it, not while they are still typing the first half
  // of an address; from then on it follows what is typed.
  let left = $state({ name: false, address: false })
  $effect(() => {
    // the fields were emptied (the record was added): the next one starts quiet
    if (draft.name === '' && draft.address === '') left = { name: false, address: false }
  })
  const nameProblem = $derived(left.name ? problems.name : null)
  const addressProblem = $derived(left.address ? problems.address : null)
  // one footer for the rows: the first thing that is wrong, top to bottom, then what the backend refused
  const problem = $derived(nameProblem ?? addressProblem ?? refused)
</script>

<form
  class="record"
  novalidate
  onsubmit={(e) => {
    e.preventDefault()
    // Return in a field: what is wrong is said now
    left = { name: true, address: true }
    if (ready && !busy) onadd()
  }}
>
  <ui-list variant="inset" label="A new name">
    <ui-section
      footer={problem ?? 'The server answers for the name with the address.'}
      footer-tone={problem ? 'error' : 'note'}
      data-testid="dns-record-form"
    >
      <ui-row>
        <span class="key">Name</span>
        <ui-text-field
          variant="bare"
          label="Name"
          description={nameProblem ?? undefined}
          placeholder="nas.home"
          inputmode="url"
          autocomplete="off"
          autocapitalize="off"
          enterkeyhint="next"
          maxlength={253}
          invalid={nameProblem !== null}
          value={draft.name}
          oninput={(e) => ondraft({ name: valueOf(e) })}
          onfocusout={() => (left.name = true)}
          data-testid="dns-record-name"
        ></ui-text-field>
      </ui-row>
      <ui-row>
        <span class="key">Address</span>
        <ui-text-field
          variant="bare"
          label="Address"
          description={(addressProblem ?? refused) || undefined}
          placeholder="192.168.1.10"
          autocomplete="off"
          autocapitalize="off"
          enterkeyhint="done"
          maxlength={45}
          invalid={addressProblem !== null || (nameProblem === null && refused !== null)}
          value={draft.address}
          oninput={(e) => ondraft({ address: valueOf(e) })}
          onfocusout={() => (left.address = true)}
          data-testid="dns-record-address"
        ></ui-text-field>
      </ui-row>
      <ui-row heading="Every Name Under It" subheading={draft.name.trim() === '' ? undefined : `*.${draft.name.trim().toLowerCase()}`}>
        <ui-toggle
          slot="trailing"
          label="Every name under it answers too"
          checked={draft.subdomains}
          onchange={(e) => ondraft({ subdomains: e.detail.checked })}
          data-testid="dns-record-subdomains"
        ></ui-toggle>
      </ui-row>
    </ui-section>
  </ui-list>
  <div class="buttons">
    {#if busy}<ui-spinner label="Adding the name" data-testid="dns-record-busy"></ui-spinner>{/if}
    <ui-button type="submit" hit disabled={!ready || busy} data-testid="dns-record-add">Add</ui-button>
  </div>
</form>

<style>
  .record {
    display: flex;
    flex-direction: column;
    gap: 8px;
    /* the rows line up with the rest of the window, and the button sits close under their footer */
    --list-inset: 16px;
  }
  ui-section {
    margin-bottom: 0;
  }
  .key {
    flex: none;
    min-width: 84px;
  }
  .buttons {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    padding-inline: 16px;
  }
</style>
