<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  type Item = VosApp.Keychain.Item
  import ItemRow from './ItemRow.svelte'

  interface Args {
    item: Item
    password: string | null
    problem: string | null
    busy: boolean
    onreveal: () => void
    onhide: () => void
    onremove: () => void
  }

  const onreveal = fn<() => void>()
  const onhide = fn<() => void>()
  const onremove = fn<() => void>()

  /** One item: what the password is for and whose it is. The password is there only once it is asked for. */
  const { Story } = defineMeta({
    title: 'Molecules/ItemRow',
    component: ItemRow,
    tags: ['autodocs'],
    args: { item: { id: 3, service: 'router.home', account: 'admin' }, password: null, problem: null, busy: false, onreveal, onhide, onremove },
    beforeEach: () => {
      onreveal.mockClear()
      onhide.mockClear()
      onremove.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <ul style="width:420px;margin:0;padding:0;background:var(--bg-secondary);color:var(--label);font:var(--type-body)">
    <ItemRow {...args} />
  </ul>
{/snippet}

<Story
  name="An item"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('keychain-item-3-service')).toHaveTextContent('router.home')
    await expect(canvas.getByTestId('keychain-item-3-account')).toHaveTextContent('admin')
    await expect(canvas.queryByTestId('keychain-item-3-password')).toBeNull()
    await userEvent.click(canvas.getByTestId('keychain-item-3-reveal'))
    await expect(onreveal).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('keychain-item-3-remove'))
    await expect(onremove).toHaveBeenCalledTimes(1)
  }}
/>

<!-- The password is shown, and the button now hides it. -->
<Story
  name="With its password shown"
  args={{ password: 'tr0ub4dor&3' }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('keychain-item-3-password')).toHaveTextContent('tr0ub4dor&3')
    await userEvent.click(canvas.getByTestId('keychain-item-3-hide'))
    await expect(onhide).toHaveBeenCalledTimes(1)
    await expect(onreveal).not.toHaveBeenCalled()
  }}
/>

<!-- Another program deleted it: said at the item, which can still be taken off the list. -->
<Story
  name="Gone from the keychain"
  args={{ problem: 'The keychain no longer has this item.' }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('keychain-item-3-problem')).toHaveTextContent('The keychain no longer has this item.')
    await expect(canvas.getByTestId('keychain-item-3-remove')).toBeVisible()
  }}
/>

<Story name="While the keychain is asked" args={{ busy: true }} />

<!-- A long service and account are cut; a long password is not: every character of it is there. -->
<Story
  name="Long"
  args={{
    item: { id: 9, service: `${'a-very-long-part-of-a-name.'.repeat(4)}example.com`, account: `${'someone-with-a-long-name.'.repeat(3)}@example.com` },
    password: 'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkFkYSJ9.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c'
  }}
/>
