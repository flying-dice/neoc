<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import definition from '../../openrpc.json'
  import ProjectsPage from './ProjectsPage.svelte'
  import { Projects } from '../lib/projects.svelte'
  import { createFailingBackend, createFakeBackend, type FakeBackend } from '../lib/fakeApi'
  import { sample } from '../lib/samples'

  // the window the contract asks for, and the smallest it allows
  const app = definition['x-app']

  interface Args {
    width: number
    height: number
    /** The backend the story's window talks to: a fake in the page, never a host. A function, so each story has its own. */
    backend: () => FakeBackend
  }

  /** The Projects app in its window, on a fake backend: every state a person can bring it to. */
  const { Story } = defineMeta({
    title: 'Pages/Projects',
    component: ProjectsPage,
    tags: ['app'],
    args: { width: app.window.w, height: app.window.h, backend: sample },
    argTypes: { backend: { control: false } },
    render: template
  })

  type Canvas = { getByTestId(id: string): HTMLElement; queryByTestId(id: string): HTMLElement | null }
  type User = { click(element: Element): Promise<void>; type(element: Element, text: string): Promise<void> }
  /** The kit's text field holds the real input in its shadow tree. */
  const input = (canvas: Canvas, id: string): HTMLInputElement => canvas.getByTestId(id).shadowRoot!.querySelector('input')!
  const appears = (canvas: Canvas, id: string): Promise<void> => waitFor(() => expect(canvas.getByTestId(id)).toBeVisible())
  const gone = (canvas: Canvas, id: string): Promise<void> => waitFor(() => expect(canvas.queryByTestId(id)).toBeNull())

  /** A segment of a kit segmented control: it is in the control's shadow tree. */
  const segment = (canvas: Canvas, control: string, value: string): Element =>
    canvas.getByTestId(control).shadowRoot!.querySelector(`[data-testid="${control}-${value}"]`)!

  /** A narrow window shows the projects first: pick one there. In a wide one the same tap only changes the project shown. */
  async function show(canvas: Canvas, user: User, id: string): Promise<void> {
    await appears(canvas, id)
    await user.click(canvas.getByTestId(id))
    await appears(canvas, 'projects-title')
  }
  async function tab(canvas: Canvas, user: User, name: string): Promise<void> {
    await user.click(segment(canvas, 'projects-tab', name))
  }
</script>

{#snippet template(args: Args)}
  <!-- one state per story, as each window has one -->
  {@const backend = args.backend()}
  {@const projects = new Projects(backend.rpc, backend.events)}
  <AppWindow title={definition.info.title} width={args.width} height={args.height} minWidth={app.window.minW} minHeight={app.window.minH}>
    <ProjectsPage {projects} />
  </AppWindow>
{/snippet}

<!-- A new workspace: no project, and the way to start one. -->
<Story
  name="Just opened"
  args={{ backend: () => createFakeBackend() }}
  play={async ({ canvas }) => {
    await appears(canvas, 'projects-new')
  }}
/>

<!-- A project's board: its issues by column, the most pressing first. -->
<Story
  name="The board"
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'projects-project-WEB')
    await expect(canvas.getByTestId('projects-title')).toHaveTextContent('Website')
    await expect(canvas.getByTestId('projects-column-todo-count')).toHaveTextContent('3')
    await expect(canvas.getByTestId('projects-column-doing-count')).toHaveTextContent('1')
    await expect(canvas.getByTestId('projects-column-done-count')).toHaveTextContent('1')
  }}
/>

<!-- The button on a card moves it one column on. -->
<Story
  name="Moving an issue"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'projects-project-WEB')
    await userEvent.click(canvas.getByTestId('projects-card-WEB-2-advance'))
    await waitFor(() => expect(canvas.getByTestId('projects-column-doing-count')).toHaveTextContent('2'))
    await expect(canvas.getByTestId('projects-column-todo-count')).toHaveTextContent('2')
  }}
/>

<!-- An issue typed on the board is added to do, and the field is ready for the next. -->
<Story
  name="Adding an issue"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'projects-project-WEB')
    await userEvent.type(input(canvas, 'projects-new-issue-input'), 'Add a contact form{Enter}')
    await waitFor(() => expect(canvas.getByTestId('projects-column-todo-count')).toHaveTextContent('4'))
    await expect(canvas.getByTestId('projects-card-WEB-6-key')).toHaveTextContent('WEB-6')
  }}
/>

<Story
  name="The issues"
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'projects-project-WEB')
    await tab(canvas, userEvent, 'issues')
    await appears(canvas, 'projects-issue-row-WEB-1')
    await expect(canvas.getByTestId('projects-issue-row-WEB-3')).toHaveProperty('value', 'In Progress')
  }}
/>

<!-- An issue, open: every part of it can be changed in place. -->
<Story
  name="An issue"
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'projects-project-WEB')
    await userEvent.click(canvas.getByTestId('projects-card-WEB-2-open'))
    await appears(canvas, 'projects-issue-key')
    await expect(input(canvas, 'projects-issue-title')).toHaveValue('Fix the footer links')
    await expect(canvas.getByTestId('projects-issue-status')).toHaveProperty('value', 'todo')
  }}
/>

<Story
  name="Changing an issue"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'projects-project-WEB')
    await userEvent.click(canvas.getByTestId('projects-card-WEB-2-open'))
    await appears(canvas, 'projects-issue-status')
    await userEvent.click(segment(canvas, 'projects-issue-status', 'done'))
    await waitFor(() => expect(canvas.getByTestId('projects-issue-status')).toHaveProperty('value', 'done'))
    await userEvent.click(canvas.getByTestId('projects-issue-back'))
    await waitFor(() => expect(canvas.getByTestId('projects-column-done-count')).toHaveTextContent('2'))
  }}
/>

<Story
  name="The pages"
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'projects-project-WEB')
    await tab(canvas, userEvent, 'pages')
    await appears(canvas, 'projects-page-row-1')
    await expect(canvas.getByTestId('projects-page-row-2')).toHaveProperty('subheading', 'In Design')
  }}
/>

<!-- A page, read: its text, and the pages under it. -->
<Story
  name="A page"
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'projects-project-WEB')
    await tab(canvas, userEvent, 'pages')
    await appears(canvas, 'projects-page-row-1')
    await userEvent.click(canvas.getByTestId('projects-page-row-1'))
    await appears(canvas, 'projects-page-body')
    await expect(canvas.getByTestId('projects-page-title')).toHaveTextContent('Design')
    await expect(canvas.getByTestId('projects-subpage-2')).toBeVisible()
  }}
/>

<!-- A page named in the list opens ready to be written; Done keeps what was typed. -->
<Story
  name="Writing a new page"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'projects-project-WEB')
    await tab(canvas, userEvent, 'pages')
    await userEvent.type(input(canvas, 'projects-new-page-input'), 'Glossary{Enter}')
    await appears(canvas, 'projects-page-text')
    await userEvent.type(canvas.getByTestId('projects-page-text').shadowRoot!.querySelector('textarea')!, 'Words we use.')
    await userEvent.click(canvas.getByTestId('projects-page-done'))
    await appears(canvas, 'projects-page-body')
    await expect(canvas.getByTestId('projects-page-body')).toHaveTextContent('Words we use.')
    await expect(canvas.getByTestId('projects-page-title')).toHaveTextContent('Glossary')
  }}
/>

<!-- A project is named in a form; it is shown as soon as it is there. -->
<Story
  name="A new project"
  args={{ backend: () => createFakeBackend() }}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'projects-new')
    await userEvent.click(canvas.getByTestId('projects-new'))
    await appears(canvas, 'projects-form-name')
    await expect(canvas.getByTestId('projects-form-add')).toHaveProperty('disabled', true)
    await userEvent.type(input(canvas, 'projects-form-name'), 'Web Site')
    await expect(input(canvas, 'projects-form-key')).toHaveValue('WS')
    await expect(canvas.getByTestId('projects-form-add')).toHaveProperty('disabled', false)
  }}
/>

<Story
  name="Adding a project"
  tags={['play-only']}
  args={{ backend: () => createFakeBackend() }}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'projects-new')
    await userEvent.click(canvas.getByTestId('projects-new'))
    await appears(canvas, 'projects-form-name')
    await userEvent.type(input(canvas, 'projects-form-name'), 'Web Site{Enter}')
    await appears(canvas, 'projects-title')
    await expect(canvas.getByTestId('projects-key')).toHaveTextContent('WS')
    await expect(canvas.queryByTestId('projects-failure')).toBeNull()
  }}
/>

<!-- A key another project has is said at the form, never as a banner. -->
<Story
  name="A key that is taken"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'projects-new')
    await userEvent.click(canvas.getByTestId('projects-new'))
    await appears(canvas, 'projects-form-name')
    await userEvent.type(input(canvas, 'projects-form-name'), 'Web')
    await userEvent.click(canvas.getByTestId('projects-form-add'))
    await waitFor(() => expect(canvas.getByTestId('projects-form')).toHaveProperty('footer', 'Another project has the key WEB.'))
    await expect(canvas.getByTestId('projects-form')).toHaveProperty('footerTone', 'error')
    await expect(canvas.queryByTestId('projects-failure')).toBeNull()
  }}
/>

<!-- Deleting a project asks first; then it, its issues and its pages are gone. -->
<Story
  name="Deleting a project"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'projects-project-APP')
    await userEvent.click(canvas.getByTestId('projects-delete'))
    await userEvent.click(canvas.getByTestId('projects-confirm').shadowRoot!.querySelector<HTMLElement>('[part=action]')!)
    await gone(canvas, 'projects-project-APP')
  }}
/>

<!-- The backend did not answer (the host, the connection): the reason, and nothing made up. -->
<Story
  name="The backend cannot be reached"
  args={{ backend: () => createFailingBackend('The host is not answering') }}
  play={async ({ canvas }) => {
    await waitFor(() => expect(canvas.getByTestId('projects-failure')).toHaveTextContent('The host is not answering'))
    await expect(canvas.queryByTestId('projects-empty')).toBeNull()
  }}
/>
