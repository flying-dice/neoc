<!-- @organism CopilotForm — the form a copilot is added or changed in: its name, its agent's command and folders, its Dockerfile and how its image stands (with the build's log), its environment, MCP servers and skills, and the way to remove it -->
<script lang="ts">
  type Copilot = VosApp.Copilot.Copilot
  type Secret = VosApp.Copilot.Secret
  type Skill = VosApp.Copilot.Skill
  import VariableRow from '../atoms/VariableRow.svelte'
  import BuildLog from '../molecules/BuildLog.svelte'
  import ServerSection from '../molecules/ServerSection.svelte'
  import type { Build } from '../lib/copilot.svelte'
  import { blankServer, blankVariable, type CopilotDraft } from '../lib/form'

  interface Props {
    form: CopilotDraft
    /** The copilot the form changes, as the backend has it now (its image, its build); null for a new one. */
    copilot: Copilot | null
    /** The skills of the library it can use. */
    skills: readonly Skill[]
    /** The Keychain's items a value can be. */
    secrets: readonly Secret[]
    /** Why the Keychain's items could not be read; values can still be written out. */
    secretsProblem?: string | null
    /** The last build of its image this window heard of. */
    build: Build | null
    /** Why the backend refused the form, shown over the fields. */
    failure?: string | null
    savable: boolean
    onsave: () => void
    onbuild: () => void
    oncancel: () => void
    onremove: () => void
  }

  let { form = $bindable(), copilot, skills, secrets, secretsProblem = null, build, failure = null, savable, onsave, onbuild, oncancel, onremove }: Props = $props()

  const valueOf = (e: Event): string => (e.currentTarget as unknown as { value: string }).value
  const adding = $derived(form.copilot === null)
  const building = $derived(build?.status === 'running' || copilot?.building === true)

  function pick(skill: string, on: boolean): void {
    form.skills = on ? [...form.skills, skill] : form.skills.filter((s) => s !== skill)
  }

  // a refusal is said at the top of the form: it is scrolled to, wherever the person was
  let fields = $state<HTMLElement>()
  $effect(() => {
    if (failure) fields?.scrollTo({ top: 0, behavior: 'smooth' })
  })
</script>

<form
  class="form"
  aria-label={adding ? 'Add a copilot' : 'Change the copilot'}
  data-testid="copilot-form"
  onsubmit={(e) => {
    e.preventDefault()
    onsave()
  }}
>
  <header class="bar">
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="plain" hit onclick={oncancel} data-testid="copilot-form-cancel">Cancel</ui-button>
    <h1>{adding ? 'New copilot' : form.copilot?.name}</h1>
    <ui-button type="submit" variant="plain" hit disabled={!savable} data-testid="copilot-form-save">{adding ? 'Add' : 'Save'}</ui-button>
  </header>
  <div class="fields" bind:this={fields}>
    {#if failure}
      <ui-banner tone="error" data-testid="copilot-form-failure">{failure}</ui-banner>
    {/if}
    <ui-list variant="inset" label="The copilot">
      <ui-section header="Copilot" footer="The command speaks ACP on its standard input and output. Each chat gets a volume of its own at the work folder." data-testid="copilot-form-copilot">
        <ui-row>
          <span class="key">Name</span>
          <ui-text-field
            label="Name"
            placeholder="Coder"
            maxlength={80}
            autocomplete="off"
            variant="bare"
            value={form.name}
            oninput={(e: Event) => (form.name = valueOf(e))}
            data-testid="copilot-form-name"
          ></ui-text-field>
        </ui-row>
        <ui-row>
          <span class="key">Command</span>
          <ui-text-field
            class="mono"
            label="Agent command"
            placeholder="claude-code-acp"
            autocapitalize="off"
            autocomplete="off"
            variant="bare"
            value={form.command}
            oninput={(e: Event) => (form.command = valueOf(e))}
            data-testid="copilot-form-command"
          ></ui-text-field>
        </ui-row>
        <ui-row>
          <span class="key">Work folder</span>
          <ui-text-field
            class="mono"
            label="Work folder"
            placeholder="/workspace"
            autocapitalize="off"
            autocomplete="off"
            maxlength={4000}
            variant="bare"
            value={form.workdir}
            oninput={(e: Event) => (form.workdir = valueOf(e))}
            data-testid="copilot-form-workdir"
          ></ui-text-field>
        </ui-row>
        <ui-row>
          <span class="key">Skills folder</span>
          <ui-text-field
            class="mono"
            label="Skills folder"
            placeholder="/root/.claude/skills"
            autocapitalize="off"
            autocomplete="off"
            maxlength={4000}
            variant="bare"
            value={form.skillsDir}
            oninput={(e: Event) => (form.skillsDir = valueOf(e))}
            data-testid="copilot-form-skills-dir"
          ></ui-text-field>
        </ui-row>
      </ui-section>

      <ui-section header="Dockerfile" footer="The whole build context: what the image needs, it downloads.">
        <ui-row>
          <ui-text-area
            label="Dockerfile"
            placeholder={'FROM oven/bun:1\nRUN bun add -g @zed-industries/claude-code-acp'}
            rows={8}
            maxlength={100000}
            autocapitalize="off"
            variant="bare"
            value={form.dockerfile}
            oninput={(e: Event) => (form.dockerfile = valueOf(e))}
            data-testid="copilot-form-dockerfile"
          ></ui-text-area>
        </ui-row>
      </ui-section>

      <ui-section header="Image" footer="Build saves the copilot, then builds its image from the Dockerfile." data-testid="copilot-form-image">
        <ui-row>
          <BuildLog {copilot} {build} testid="copilot-build" />
        </ui-row>
        <ui-row heading={building ? 'Building…' : 'Build'} action tappable disabled={building || !savable} onpress={onbuild} data-testid="copilot-form-build"></ui-row>
      </ui-section>

      <ui-section
        header="Environment"
        footer={secretsProblem ?? "A value can be a Keychain item's password, read when a chat's container is made."}
        footer-tone={secretsProblem ? 'error' : 'note'}
        data-testid="copilot-form-env"
      >
        {#each form.env as variable, i (variable.key)}
          <VariableRow
            bind:variable={form.env[i]!}
            kind="Variable"
            {secrets}
            testid="copilot-form-env-{i}"
            onremove={() => (form.env = form.env.filter((v) => v.key !== variable.key))}
          />
        {/each}
        <ui-row heading="Add a variable" action tappable onpress={() => form.env.push(blankVariable())} data-testid="copilot-form-add-env"></ui-row>
      </ui-section>

      {#each form.mcp as server, i (server.key)}
        <ServerSection
          bind:server={form.mcp[i]!}
          {secrets}
          testid="copilot-form-server-{i}"
          onremove={() => (form.mcp = form.mcp.filter((s) => s.key !== server.key))}
        />
      {/each}
      <ui-section header={form.mcp.length === 0 ? 'MCP servers' : undefined}>
        <ui-row heading="Add a program server" action tappable onpress={() => form.mcp.push(blankServer('stdio'))} data-testid="copilot-form-add-stdio"></ui-row>
        <ui-row heading="Add a URL server" action tappable onpress={() => form.mcp.push(blankServer('http'))} data-testid="copilot-form-add-http"></ui-row>
      </ui-section>

      <ui-section
        header="Skills"
        footer="Copied into each new chat's container, in the skills folder."
        data-testid="copilot-form-skills"
      >
        {#if skills.length === 0}
          <ui-row heading="The skill library is empty."></ui-row>
        {/if}
        {#each skills as skill (skill.id)}
          <ui-row heading={skill.name}>
            <ui-check
              slot="trailing"
              label={skill.name}
              checked={form.skills.includes(skill.id)}
              onchange={(e) => pick(skill.id, e.detail.checked)}
              data-testid="copilot-form-skill-{skill.id}"
            ></ui-check>
          </ui-row>
        {/each}
      </ui-section>

      {#if !adding}
        <ui-section footer="Its chats, with their containers and volumes, and its images go with it.">
          <ui-row heading="Remove this copilot" action destructive tappable onpress={onremove} data-testid="copilot-form-remove"></ui-row>
        </ui-section>
      {/if}
    </ui-list>
  </div>
</form>

<style>
  .form {
    box-sizing: border-box;
    height: 100%;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
    display: flex;
    flex-direction: column;
    background: var(--bg-secondary);
  }
  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 0 8px 6px;
    border-bottom: 1px solid var(--separator);
  }
  h1 {
    margin: 0;
    min-width: 0;
    font: var(--type-headline);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .fields {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 16px 12px;
  }
  ui-banner {
    display: block;
    margin: 0 0 12px;
  }
  ui-row {
    --row-gap: 12px;
  }
  .key {
    flex: none;
    width: 7em;
  }
  ui-text-field {
    flex: 1;
    min-width: 0;
  }
  .mono {
    font-family: var(--font-mono);
  }
  ui-text-area {
    flex: 1;
    min-width: 0;
  }
  ui-text-area::part(input) {
    font-family: var(--font-mono);
    font-size: 13px;
  }
</style>
