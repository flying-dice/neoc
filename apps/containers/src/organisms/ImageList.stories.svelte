<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import { alertButton, alertDialog } from '@vos/storybook/kit'
  type Image = VosApp.Containers.Image
  type Recipe = VosApp.Containers.Recipe
  import ImageList from './ImageList.svelte'
  import { statusOf, type RecipeStatus } from '../lib/recipes.svelte'
  import type { Pull } from '../lib/resources.svelte'
  import { IMAGES, image, recipe } from '../lib/fakeApi'

  interface Args {
    images: readonly Image[]
    pull: Pull
    pruning?: boolean
    report?: string
    back?: boolean
    onback?: () => void
    onpull: () => void
    onremove: (image: Image) => void
    onnew: (image: Image) => void
    onprune: () => void
    recipes: readonly Recipe[]
    statusOf: (recipe: Recipe) => RecipeStatus
    onrecipe: (recipe: Recipe) => void
    onnewrecipe: () => void
  }

  const onback = fn<() => void>()
  const onpull = fn<() => void>()
  const onremove = fn<(image: Image) => void>()
  const onnew = fn<(image: Image) => void>()
  const onprune = fn<() => void>()
  const onrecipe = fn<(recipe: Recipe) => void>()
  const onnewrecipe = fn<() => void>()
  const heard = [onback, onpull, onremove, onnew, onprune, onrecipe, onnewrecipe]

  // the images of the fake's machine, each with the containers made from it as the backend lists them
  const USERS: Readonly<Record<string, string[]>> = {
    'postgres:16': ['blog-db-1'],
    'ghost:5': ['blog-web-1'],
    'alpine:3.20': ['scratch'],
    'pihole/pihole:2024.07.0': ['pihole']
  }
  const LISTED: Image[] = IMAGES.map((i) => ({ ...i, containers: USERS[i.tags[0] ?? ''] ?? [] }))
  // a recipe built from what it is now, one whose last build failed, and one never built
  const BUILT = { ...image(['tools:1']), containers: [] }
  const RECIPES: Recipe[] = [
    recipe('tools:1', { image: BUILT.id, current: true }),
    recipe('broken:1', { problem: "The image could not be built: The command '/bin/sh -c exit 3' returned a non-zero code: 3" }),
    recipe('later:1')
  ]
  const idle = (more: Partial<Pull> = {}): Pull => ({ reference: '', busy: false, progress: null, failure: null, ...more })
  const input = (element: HTMLElement): HTMLInputElement => element.shadowRoot!.querySelector('input')!

  /** The images Docker has. It keeps only the question it asks before a prune; what is typed goes into the pull it was given. */
  const { Story } = defineMeta({
    title: 'Organisms/ImageList',
    component: ImageList,
    tags: ['autodocs'],
    args: {
      images: LISTED,
      pull: idle(),
      onback,
      onpull,
      onremove,
      onnew,
      onprune,
      recipes: [],
      statusOf: (r: Recipe) => statusOf(r, null, LISTED),
      onrecipe,
      onnewrecipe
    },
    beforeEach: () => heard.forEach((spy) => spy.mockClear()),
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:600px;height:620px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <ImageList {...args} />
  </div>
{/snippet}

<!-- Every image: its id, size and day, how many containers were made from it; a dangling one has no name, and nothing to make a container of. -->
<Story
  name="Images"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-title')).toHaveTextContent('Images')
    await expect(canvas.getByTestId('containers-summary')).toHaveTextContent('8 images')
    await expect(canvas.getByTestId('containers-image-postgres:16-note')).toHaveTextContent('In use by 1 container: blog-db-1')
    await expect(canvas.getByTestId('containers-image-redis:7')).toHaveTextContent('also redis:latest')
    await expect(canvas.getByTestId('containers-image-redis:7-note')).toHaveTextContent('Unused')
    const dangling = LISTED.find((i) => i.tags.length === 0)!
    const id = dangling.id.slice(7, 19)
    await expect(canvas.getByTestId(`containers-image-${id}`)).toHaveTextContent('<none>:<none>')
    await expect(canvas.queryByTestId(`containers-image-<none>:<none>-new`)).toBeNull()
    await expect(canvas.getByTestId('containers-images-prune')).toHaveProperty('disabled', false)
  }}
/>

<!-- The recipes images are built from, each with how its image stands. -->
<Story
  name="Recipes"
  args={{ images: [...LISTED, BUILT], recipes: RECIPES, statusOf: (r: Recipe) => statusOf(r, null, [...LISTED, BUILT]) }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-recipe-tools:1')).toHaveProperty('subheading', `Built: ${BUILT.id.slice(7, 19)}`)
    await expect(canvas.getByTestId('containers-recipe-broken:1')).toHaveProperty('subheading', expect.stringContaining('Failed: The image could not be built'))
    await expect(canvas.getByTestId('containers-recipe-later:1')).toHaveProperty('subheading', 'Not built yet.')
    await expect(canvas.getByTestId('containers-recipe-new')).toBeVisible()
  }}
/>

<!-- Docker has no image: nothing to prune. -->
<Story
  name="No images"
  args={{ images: [] }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-images-empty')).toBeVisible()
    await expect(canvas.getByTestId('containers-summary')).toHaveTextContent('No images')
    await expect(canvas.getByTestId('containers-images-prune')).toHaveProperty('disabled', true)
  }}
/>

<!-- An image is pulled by its name: the same meter as the form's, while Docker downloads its layers. -->
<Story
  name="Pulling an image"
  args={{ pull: idle({ reference: 'nginx:1.27', busy: true, progress: { image: 'nginx:1.27', status: 'Downloading', current: 24_000_000, total: 48_000_000 } }) }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-pull-progress')).toHaveTextContent('24.0 MB of 48.0 MB')
    await expect(canvas.getByTestId('containers-pull')).toHaveProperty('disabled', true)
  }}
/>

<!-- The registry does not have it: the backend's reason, under the field, and the name stays. -->
<Story
  name="A pull that is refused"
  args={{
    pull: idle({
      reference: 'nosuch/app:1',
      failure: "The image nosuch/app:1 could not be pulled: pull access denied for nosuch/app, repository does not exist or may require 'docker login'"
    })
  }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-pull-about')).toHaveAttribute('footer-tone', 'error')
    await expect(canvas.getByTestId('containers-pull-about')).toHaveProperty('footer', expect.stringContaining('pull access denied'))
    await expect(input(canvas.getByTestId('containers-pull-image')).value).toBe('nosuch/app:1')
  }}
/>

<!-- A prune asks first, saying how much goes. -->
<Story
  name="Pruning asks first"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('containers-images-prune'))
    await expect(alertDialog(canvas.getByTestId('containers-images-prune-confirm'))).toHaveTextContent('Remove 1 dangling image?')
    await expect(alertDialog(canvas.getByTestId('containers-images-prune-confirm'))).toHaveTextContent('140 MB')
  }}
/>

<!-- What the prune freed, once it is done. -->
<Story
  name="Pruned"
  args={{ images: LISTED.filter((i) => i.tags.length > 0), report: 'Removed 1 dangling image, and freed 140 MB.' }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-images-report')).toHaveTextContent('freed 140 MB')
    await expect(canvas.getByTestId('containers-images-prune')).toHaveProperty('disabled', true)
  }}
/>

<Story
  name="Tapping"
  tags={['play-only']}
  args={{ images: [{ ...image(['nginx:1.27']), containers: [] }], pull: idle({ reference: 'redis:7' }) }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('containers-pull'))
    await expect(onpull).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('containers-image-nginx:1.27-new'))
    await expect(onnew).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('containers-image-nginx:1.27-remove'))
    await expect(onremove).toHaveBeenCalledTimes(1)
  }}
/>

<Story
  name="Tapping a recipe"
  tags={['play-only']}
  args={{ recipes: RECIPES }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('containers-recipe-later:1'))
    await expect(onrecipe).toHaveBeenCalledWith(RECIPES[2])
    await userEvent.click(canvas.getByTestId('containers-recipe-new'))
    await expect(onnewrecipe).toHaveBeenCalledTimes(1)
  }}
/>

<!-- Saying yes to the prune prunes; saying no does nothing. -->
<Story
  name="Answering the prune"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('containers-images-prune'))
    await userEvent.click(alertButton(canvas.getByTestId('containers-images-prune-confirm'), 'cancel'))
    await expect(onprune).not.toHaveBeenCalled()
    await userEvent.click(canvas.getByTestId('containers-images-prune'))
    await userEvent.click(alertButton(canvas.getByTestId('containers-images-prune-confirm'), 'action'))
    await expect(onprune).toHaveBeenCalledTimes(1)
  }}
/>
