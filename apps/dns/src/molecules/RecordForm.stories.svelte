<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import RecordForm from './RecordForm.svelte'
  import type { Problems, RecordDraft } from '../lib/dns.svelte'

  interface Args {
    draft: RecordDraft
    problems: Problems<'name' | 'address'>
    refused: string | null
    busy: boolean
    ondraft: (change: Partial<RecordDraft>) => void
    onadd: () => void
  }

  const ondraft = fn<(change: Partial<RecordDraft>) => void>()
  const onadd = fn<() => void>()

  /** The next record as it is typed. Add cannot be pressed until there is a name and an address with nothing wrong with them. */
  const { Story } = defineMeta({
    title: 'Molecules/RecordForm',
    component: RecordForm,
    tags: ['autodocs'],
    args: { draft: { name: '', address: '', subdomains: false }, problems: { name: null, address: null }, refused: null, busy: false, ondraft, onadd },
    beforeEach: () => {
      ondraft.mockClear()
      onadd.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:520px;padding:16px 0;background:var(--bg);color:var(--label);font:var(--type-body)">
    <RecordForm {...args} />
  </div>
{/snippet}

<!-- Nothing to add yet: Add cannot be pressed, and the footer says what the rows are for. -->
<Story
  name="Empty"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('dns-record-add')).toHaveProperty('disabled', true)
    await expect(canvas.getByTestId('dns-record-form')).toHaveProperty('footerTone', 'note')
    await userEvent.click(canvas.getByTestId('dns-record-add'))
    await expect(onadd).not.toHaveBeenCalled()
  }}
/>

<Story
  name="Typed"
  args={{ draft: { name: 'nas.home', address: '192.168.1.10', subdomains: true } }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('dns-record-add'))
    await expect(onadd).toHaveBeenCalledTimes(1)
  }}
/>

<!-- A field that is wrong is quiet until the person leaves it: then the footer says what is wrong, the first thing first, and Add cannot be pressed. -->
<Story
  name="A name and an address that are none"
  args={{
    draft: { name: 'nas_home', address: '192.168.1', subdomains: false },
    problems: { name: 'Only letters, digits, hyphens and dots, like nas.home.', address: 'Not an IP address, like 192.168.1.10 or fd00::10.' }
  }}
  play={async ({ canvas, userEvent }) => {
    const form = canvas.getByTestId('dns-record-form')
    await expect(form).toHaveProperty('footerTone', 'note')
    await userEvent.click(canvas.getByTestId('dns-record-name').shadowRoot!.querySelector('input')!)
    await userEvent.tab()
    await expect(form).toHaveProperty('footer', 'Only letters, digits, hyphens and dots, like nas.home.')
    await expect(form).toHaveProperty('footerTone', 'error')
    await expect(canvas.getByTestId('dns-record-name')).toHaveProperty('invalid', true)
    await expect(canvas.getByTestId('dns-record-add')).toHaveProperty('disabled', true)
    await userEvent.click(canvas.getByTestId('dns-record-add'))
    await expect(onadd).not.toHaveBeenCalled()
  }}
/>

<!-- What only the backend can know (the name has that address already) is said in the same place. -->
<Story
  name="Refused by the backend"
  args={{ draft: { name: 'nas.home', address: '192.168.1.10', subdomains: false }, refused: 'nas.home has the address 192.168.1.10 already.' }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('dns-record-form')).toHaveProperty('footer', 'nas.home has the address 192.168.1.10 already.')
    await expect(canvas.getByTestId('dns-record-form')).toHaveProperty('footerTone', 'error')
  }}
/>

<Story name="While a change is in flight" args={{ draft: { name: 'nas.home', address: '192.168.1.10', subdomains: false }, busy: true }} />
