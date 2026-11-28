<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import { alertButton, alertDialog } from '@vos/storybook/kit'
  type Recipe = VosApp.Containers.Recipe
  import RecipeForm from './RecipeForm.svelte'
  import { blankRecipe, type RecipeDraft, type RecipeForm as Form, type RecipeStatus } from '../lib/recipes.svelte'
  import { recipe } from '../lib/fakeApi'

  interface Args {
    form: Form
    recipe: Recipe | null
    status: RecipeStatus
    lines: readonly string[]
    savable: boolean
    onsave: () => void
    onbuild: () => void
    oncancel: () => void
    onremove: () => void
  }

  const onsave = fn<() => void>()
  const onbuild = fn<() => void>()
  const oncancel = fn<() => void>()
  const onremove = fn<() => void>()
  const heard = [onsave, onbuild, oncancel, onremove]

  const TOOLS = recipe('tools:1', { image: `sha256:${'1d34ffeaf190'.repeat(5)}abcd`, current: true })
  const form = (draft: RecipeDraft, more: Partial<Form> = {}): Form => ({ recipe: null, draft, failure: null, saving: false, ...more })
  const of = (r: Recipe, more: Partial<Form> = {}): Form => form({ tag: r.tag, dockerfile: r.dockerfile }, { recipe: r.id, ...more })
  const input = (element: HTMLElement): HTMLInputElement => element.shadowRoot!.querySelector('input')!
  const area = (element: HTMLElement): HTMLTextAreaElement => element.shadowRoot!.querySelector('textarea')!

  /** The form a recipe is kept and built in. It keeps only the question it asks before a removal; what is typed goes into the form it was given. */
  const { Story } = defineMeta({
    title: 'Organisms/RecipeForm',
    component: RecipeForm,
    tags: ['autodocs'],
    args: { form: form(blankRecipe()), recipe: null, status: { tone: 'none', text: 'Not built yet.' }, lines: [], savable: false, onsave, onbuild, oncancel, onremove },
    beforeEach: () => heard.forEach((spy) => spy.mockClear()),
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:560px;height:640px;color:var(--label);font:var(--type-body)">
    <RecipeForm {...args} />
  </div>
{/snippet}

<!-- A new recipe: nothing to save or build until it names an image and has a Dockerfile, and nothing to remove. -->
<Story
  name="New"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-recipe-form-title')).toHaveTextContent('New Recipe')
    await expect(canvas.getByTestId('containers-recipe-form-save')).toHaveProperty('disabled', true)
    await expect(canvas.getByTestId('containers-recipe-form-build')).toHaveProperty('disabled', true)
    await expect(canvas.queryByTestId('containers-recipe-form-remove')).toBeNull()
  }}
/>

<!-- A recipe built from what it is now: its name, its Dockerfile, and the image it built. -->
<Story
  name="Built"
  args={{ form: of(TOOLS), recipe: TOOLS, status: { tone: 'done', text: 'Built: 1d34ffeaf190' }, savable: true }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-recipe-form-title')).toHaveTextContent('tools:1')
    await expect(input(canvas.getByTestId('containers-recipe-form-tag')).value).toBe('tools:1')
    await expect(area(canvas.getByTestId('containers-recipe-form-dockerfile')).value).toContain('FROM alpine:3.20')
    await expect(canvas.getByTestId('containers-recipe-log-status')).toHaveTextContent('Built: 1d34ffeaf190')
  }}
/>

<!-- Under way: the log fills, and Build waits for it to end. -->
<Story
  name="Building"
  args={{
    form: of(TOOLS),
    recipe: { ...TOOLS, building: true },
    status: { tone: 'running', text: 'Building…' },
    lines: ['Building tools:1', 'Step 1/2 : FROM alpine:3.20'],
    savable: true
  }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-recipe-log-lines')).toHaveTextContent('Step 1/2 : FROM alpine:3.20')
    await expect(canvas.getByTestId('containers-recipe-form-build')).toHaveProperty('disabled', true)
  }}
/>

<!-- The backend refused what was typed: its reason at the top, and the form as it was. -->
<Story
  name="Refused"
  args={{ form: form({ tag: 'Tools', dockerfile: 'FROM alpine' }, { failure: 'Tools is not the name of an image. An image is named like tools, tools:1 or ghcr.io/owner/tools:1.' }), savable: true }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-recipe-form-failure')).toHaveTextContent('Tools is not the name of an image.')
  }}
/>

<Story
  name="Tapping"
  tags={['play-only']}
  args={{ form: of(TOOLS), recipe: TOOLS, savable: true }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('containers-recipe-form-build'))
    await expect(onbuild).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('containers-recipe-form-save'))
    await expect(onsave).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('containers-recipe-form-cancel'))
    await expect(oncancel).toHaveBeenCalledTimes(1)
  }}
/>

<!-- Removing asks first, and says the images stay; no does nothing. -->
<Story
  name="Removing asks first"
  tags={['play-only']}
  args={{ form: of(TOOLS), recipe: TOOLS, savable: true }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('containers-recipe-form-remove'))
    await expect(alertDialog(canvas.getByTestId('containers-recipe-form-remove-confirm'))).toHaveTextContent('The images it built stay.')
    await userEvent.click(alertButton(canvas.getByTestId('containers-recipe-form-remove-confirm'), 'cancel'))
    await expect(onremove).not.toHaveBeenCalled()
    await userEvent.click(canvas.getByTestId('containers-recipe-form-remove'))
    await userEvent.click(alertButton(canvas.getByTestId('containers-recipe-form-remove-confirm'), 'action'))
    await expect(onremove).toHaveBeenCalledTimes(1)
  }}
/>
