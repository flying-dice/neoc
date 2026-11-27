<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import { pickerSelect } from '@vos/storybook/kit'
  import NetworkForm from './NetworkForm.svelte'
  import { blankNetwork, type NetworkDraft, type ObjectForm } from '../lib/resources.svelte'
  import { blankEntry } from '../lib/spec'

  interface Args {
    form: ObjectForm<NetworkDraft>
    savable: boolean
    onsave: () => void
    oncancel: () => void
  }

  const onsave = fn<() => void>()
  const oncancel = fn<() => void>()

  const form = (draft: NetworkDraft, more: Partial<ObjectForm<NetworkDraft>> = {}): ObjectForm<NetworkDraft> => ({ draft, failure: null, saving: false, ...more })
  const filled = (): NetworkDraft => ({
    name: 'backend',
    driver: 'bridge',
    subnet: '172.30.0.0/16',
    gateway: '172.30.0.1',
    internal: true,
    attachable: false,
    labels: [{ ...blankEntry(), name: 'com.example.team', value: 'web' }]
  })
  const input = (element: HTMLElement): HTMLInputElement => element.shadowRoot!.querySelector('input')!

  /** The form a network is made in. It keeps nothing: what is typed goes into the form it was given. */
  const { Story } = defineMeta({
    title: 'Organisms/NetworkForm',
    component: NetworkForm,
    tags: ['autodocs'],
    args: { form: form(blankNetwork()), savable: false, onsave, oncancel },
    beforeEach: () => {
      onsave.mockClear()
      oncancel.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:520px;height:640px;color:var(--label);font:var(--type-body)">
    <NetworkForm {...args} />
  </div>
{/snippet}

<!-- A new network: a bridge, with addresses Docker chooses, until it is named. -->
<Story
  name="Empty"
  play={async ({ canvas }) => {
    await expect(pickerSelect(canvas.getByTestId('containers-network-form-driver')).value).toBe('bridge')
    await expect(canvas.getByTestId('containers-network-form-internal')).toHaveProperty('checked', false)
    await expect(canvas.getByTestId('containers-network-form-save')).toHaveProperty('disabled', true)
  }}
/>

<!-- Its own subnet and gateway, kept to itself, and labelled. -->
<Story
  name="Filled"
  args={{ form: form(filled()), savable: true }}
  play={async ({ canvas, userEvent }) => {
    await expect(input(canvas.getByTestId('containers-network-form-subnet')).value).toBe('172.30.0.0/16')
    await expect(canvas.getByTestId('containers-network-form-internal')).toHaveProperty('checked', true)
    await userEvent.click(canvas.getByTestId('containers-network-form-save'))
    await expect(onsave).toHaveBeenCalledTimes(1)
  }}
/>

<!-- The backend refused: its reason at the top, and the form as it was. -->
<Story
  name="Refused"
  args={{ form: form({ ...filled(), subnet: '10.0.0.0/8' }, { failure: 'Pool overlaps with other one on this address space' }), savable: true }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-network-form-failure')).toHaveTextContent('Pool overlaps with other one on this address space')
  }}
/>
