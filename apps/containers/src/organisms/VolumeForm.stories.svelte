<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import VolumeForm from './VolumeForm.svelte'
  import { blankVolume, type ObjectForm, type VolumeDraft } from '../lib/resources.svelte'
  import { blankEntry } from '../lib/spec'

  interface Args {
    form: ObjectForm<VolumeDraft>
    savable: boolean
    onsave: () => void
    oncancel: () => void
  }

  const onsave = fn<() => void>()
  const oncancel = fn<() => void>()

  const form = (draft: VolumeDraft, more: Partial<ObjectForm<VolumeDraft>> = {}): ObjectForm<VolumeDraft> => ({ draft, failure: null, saving: false, ...more })
  const filled = (): VolumeDraft => ({ name: 'cache', driver: 'local', labels: [{ ...blankEntry(), name: 'com.example.team', value: 'web' }] })
  const input = (element: HTMLElement): HTMLInputElement => element.shadowRoot!.querySelector('input')!

  /** The form a volume is made in. It keeps nothing: what is typed goes into the form it was given. */
  const { Story } = defineMeta({
    title: 'Organisms/VolumeForm',
    component: VolumeForm,
    tags: ['autodocs'],
    args: { form: form(blankVolume()), savable: true, onsave, oncancel },
    beforeEach: () => {
      onsave.mockClear()
      oncancel.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:520px;height:520px;color:var(--label);font:var(--type-body)">
    <VolumeForm {...args} />
  </div>
{/snippet}

<!-- A new volume: Docker's own driver, and a name Docker makes up unless one is given. -->
<Story
  name="Empty"
  play={async ({ canvas, userEvent }) => {
    await expect(input(canvas.getByTestId('containers-volume-form-driver')).value).toBe('local')
    await userEvent.click(canvas.getByTestId('containers-volume-form-add-label'))
    await expect(canvas.getByTestId('containers-volume-form-label-0')).toBeVisible()
    await userEvent.click(canvas.getByTestId('containers-volume-form-save'))
    await expect(onsave).toHaveBeenCalledTimes(1)
  }}
/>

<Story
  name="Filled"
  args={{ form: form(filled()) }}
  play={async ({ canvas }) => {
    await expect(input(canvas.getByTestId('containers-volume-form-name')).value).toBe('cache')
    await expect(input(canvas.getByTestId('containers-volume-form-label-0-name')).value).toBe('com.example.team')
  }}
/>

<!-- The backend refused: its reason at the top, and the form as it was. -->
<Story
  name="Refused"
  args={{ form: form({ ...filled(), name: 'pihole-data' }, { failure: 'There is a volume named pihole-data already.' }) }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-volume-form-failure')).toHaveTextContent('There is a volume named pihole-data already.')
  }}
/>

<Story
  name="Saving"
  args={{ form: form(filled(), { saving: true }), savable: false }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-volume-form-saving')).toBeVisible()
    await expect(canvas.getByTestId('containers-volume-form-cancel')).toHaveProperty('disabled', true)
  }}
/>
