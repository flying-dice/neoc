<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { Item } from '@keychain/rpc'
  import ItemList from './ItemList.svelte'
  import { ITEMS } from '../lib/fakeApi'
  import type { ItemDraft } from '../lib/keychain.svelte'

  interface Args {
    items: readonly Item[]
    draft: ItemDraft
    ready: boolean
    refused: string | null
    adding: boolean
    password: (item: Item) => string | null
    problem: (item: Item) => string | null
    busy: (item: Item) => boolean
    ondraft: (change: Partial<ItemDraft>) => void
    onadd: () => void
    onreveal: (item: Item) => void
    onhide: (item: Item) => void
    onremove: (item: Item) => void
  }

  const onreveal = fn<(item: Item) => void>()
  const onremove = fn<(item: Item) => void>()

  // the items as the app has them: no password among them
  const LISTED: readonly Item[] = ITEMS.map(({ id, service, account }) => ({ id, service, account }))

  /** The items of the keychain the app shows, with the form that adds one. */
  const { Story } = defineMeta({
    title: 'Organisms/ItemList',
    component: ItemList,
    tags: ['autodocs'],
    args: {
      items: LISTED,
      draft: { service: '', account: '', password: '' },
      ready: false,
      refused: null,
      adding: false,
      password: () => null,
      problem: () => null,
      busy: () => false,
      ondraft: fn(),
      onadd: fn(),
      onreveal,
      onhide: fn(),
      onremove
    },
    argTypes: { password: { control: false }, problem: { control: false }, busy: { control: false } },
    beforeEach: () => {
      onreveal.mockClear()
      onremove.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:560px;height:520px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <ItemList {...args} />
  </div>
{/snippet}

<Story
  name="A home lab"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('keychain-items-summary')).toHaveTextContent('3 items')
    await userEvent.click(canvas.getByTestId('keychain-item-3-reveal'))
    await expect(onreveal).toHaveBeenCalledWith(LISTED[2])
    await userEvent.click(canvas.getByTestId('keychain-item-2-remove'))
    await expect(onremove).toHaveBeenCalledWith(LISTED[1])
  }}
/>

<Story
  name="No items"
  args={{ items: [] }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('keychain-empty')).toBeVisible()
    await expect(canvas.getByTestId('keychain-items-summary')).toHaveTextContent('No items')
  }}
/>

<!-- One password is shown, one could not be, and the keychain is being asked for a third. -->
<Story
  name="Shown, gone and asked for"
  args={{
    password: (item: Item) => (item.id === 1 ? 'correct horse battery staple' : null),
    problem: (item: Item) => (item.id === 2 ? 'The keychain no longer has this item.' : null),
    busy: (item: Item) => item.id === 3
  }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('keychain-item-1-password')).toHaveTextContent('correct horse battery staple')
    await expect(canvas.getByTestId('keychain-item-2-problem')).toBeVisible()
    await expect(canvas.getByTestId('keychain-item-3-busy')).toBeVisible()
  }}
/>
