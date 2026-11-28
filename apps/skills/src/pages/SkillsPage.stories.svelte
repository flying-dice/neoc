<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import { alertButton, alertDialog } from '@vos/storybook/kit'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import pkg from '../../package.json'
  import SkillsPage from './SkillsPage.svelte'
  import { createFailingBackend, createFakeBackend, type FakeBackend } from '../lib/fakeApi'
  import { Skills } from '../lib/skills.svelte'

  // the window the app asks for (package.json), and the smallest it allows
  const app = pkg.vos

  interface Args {
    width: number
    height: number
    /** The backend the story's window talks to: a fake in the page, never a host. A function, so each story has its own. */
    backend: () => FakeBackend
  }

  /** The Skills app in its window, on a fake backend: every state a person can bring it to. */
  const { Story } = defineMeta({
    title: 'Pages/Skills',
    component: SkillsPage,
    tags: ['app'],
    args: { width: app.window.w, height: app.window.h, backend: () => createFakeBackend() },
    argTypes: { backend: { control: false } },
    render: template
  })

  type Canvas = { getByTestId(id: string): HTMLElement; queryByTestId(id: string): HTMLElement | null }
  /** The kit's controls hold the real one in their shadow tree. */
  const inner = (canvas: Canvas, id: string, selector: string): HTMLElement => canvas.getByTestId(id).shadowRoot!.querySelector<HTMLElement>(selector)!
  const appears = (canvas: Canvas, id: string): Promise<void> => waitFor(() => expect(canvas.getByTestId(id)).toBeVisible())
  const gone = (canvas: Canvas, id: string): Promise<void> => waitFor(() => expect(canvas.queryByTestId(id)).toBeNull())
</script>

{#snippet template(args: Args)}
  <!-- one state per story, as each window has one -->
  {@const backend = args.backend()}
  {@const skills = new Skills(backend.rpc, backend.events)}
  <AppWindow title={app.title} width={args.width} height={args.height} minWidth={app.window.minW} minHeight={app.window.minH}>
    <SkillsPage {skills} />
  </AppWindow>
{/snippet}

<!-- Two skills: one says what it is for, the other is only its SKILL.md. -->
<Story
  name="Skills"
  play={async ({ canvas }) => {
    await appears(canvas, 'skills-skill-review')
    await expect(canvas.getByTestId('skills-skill-review')).toHaveProperty('subheading', 'Review a pull request before it is merged.')
  }}
/>

<Story name="In the smallest window" args={{ width: app.window.minW, height: app.window.minH }} />

<Story name="No skills yet" args={{ backend: () => createFakeBackend({ skills: [] }) }} />

<!-- A skill is added with its SKILL.md: it is in the list, by what its front matter says it is for. -->
<Story
  name="Adding a skill"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'skills-add')
    await userEvent.click(canvas.getByTestId('skills-add'))
    await appears(canvas, 'skills-form-name')
    await userEvent.type(inner(canvas, 'skills-form-name', 'input'), 'changelog')
    await userEvent.type(inner(canvas, 'skills-form-text', 'textarea'), '---{enter}description: Write the changelog.{enter}---{enter}# Changelog')
    await userEvent.click(canvas.getByTestId('skills-form-save'))
    await appears(canvas, 'skills-skill-skill-1')
    await expect(canvas.getByTestId('skills-skill-skill-1')).toHaveProperty('subheading', 'Write the changelog.')
  }}
/>

<!-- Removing a skill asks first. -->
<Story
  name="Removing a skill"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'skills-skill-deploy')
    await userEvent.click(canvas.getByTestId('skills-skill-deploy'))
    await appears(canvas, 'skills-form-remove')
    await userEvent.click(canvas.getByTestId('skills-form-remove'))
    // the kit's alert holds its words in its shadow tree
    await waitFor(() => expect(alertDialog(canvas.getByTestId('skills-confirm'))).toHaveTextContent('Remove deploy?'))
    await userEvent.click(alertButton(canvas.getByTestId('skills-confirm'), 'action'))
    await gone(canvas, 'skills-skill-deploy')
    await appears(canvas, 'skills-skill-review')
  }}
/>

<!-- The backend cannot be reached: why. -->
<Story name="No backend" args={{ backend: () => createFailingBackend('The host is not answering.') }} />
