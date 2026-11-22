<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import ItemForm from './ItemForm.svelte'
  import type { ItemDraft } from '../lib/keychain.svelte'

  interface Args {
    draft: ItemDraft
    ready: boolean
    refused: string | null
    busy: boolean
    ondraft: (change: Partial<ItemDraft>) => void
    onadd: () => void
  }

  const ondraft = fn<(change: Partial<ItemDraft>) => void>()
  const onadd = fn<() => void>()

  const TYPED: ItemDraft = { service: 'router.home', account: 'admin', password: 'tr0ub4dor&3' }

  /** The next item as it is typed. Add cannot be pressed until there is a service and an account. */
  const { Story } = defineMeta({
    title: 'Molecules/ItemForm',
    component: ItemForm,
    tags: ['autodocs'],
    args: { draft: { service: '', account: '', password: '' }, ready: false, refused: null, busy: false, ondraft, onadd },
    beforeEach: () => {
      ondraft.mockClear()
      onadd.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:520px;padding:16px 0;background:var(--bg);color:var(--label);font:var(--type-body)">
    <ItemForm {...args} />
  </div>
{/snippet}

<!-- Nothing to add yet: Add cannot be pressed, and the footer says what an empty password means. -->
<Story
  name="Empty"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('keychain-new-add')).toHaveProperty('disabled', true)
    await expect(canvas.getByTestId('keychain-new-form')).toHaveProperty('footerTone', 'note')
    await userEvent.click(canvas.getByTestId('keychain-new-add'))
    await expect(onadd).not.toHaveBeenCalled()
  }}
/>

<!-- The password is typed out of sight. -->
<Story
  name="Typed"
  args={{ draft: TYPED, ready: true }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('keychain-new-password').shadowRoot!.querySelector('input')).toHaveAttribute('type', 'password')
    await userEvent.click(canvas.getByTestId('keychain-new-add'))
    await expect(onadd).toHaveBeenCalledTimes(1)
  }}
/>

<!-- What is typed goes out field by field. -->
<Story
  name="Typing"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await userEvent.type(canvas.getByTestId('keychain-new-service').shadowRoot!.querySelector('input')!, 'r')
    await expect(ondraft).toHaveBeenCalledWith({ service: 'r' })
    await userEvent.type(canvas.getByTestId('keychain-new-password').shadowRoot!.querySelector('input')!, 's')
    await expect(ondraft).toHaveBeenCalledWith({ password: 's' })
  }}
/>

<!-- What only the keychain can know (it has a password for this already) is said in the footer, at the password. -->
<Story
  name="Refused by the keychain"
  args={{
    draft: TYPED,
    ready: true,
    refused: 'The keychain has a password for this service and account already. Leave the password empty to show it here.'
  }}
  play={async ({ canvas }) => {
    const form = canvas.getByTestId('keychain-new-form')
    await expect(form).toHaveProperty(
      'footer',
      'The keychain has a password for this service and account already. Leave the password empty to show it here.'
    )
    await expect(form).toHaveProperty('footerTone', 'error')
    await expect(canvas.getByTestId('keychain-new-password')).toHaveProperty('invalid', true)
  }}
/>

<!-- The machine may be asking its person: nothing more is added until it has answered. -->
<Story
  name="While it is added"
  args={{ draft: TYPED, ready: true, busy: true }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('keychain-new-add')).toHaveProperty('disabled', true)
  }}
/>
