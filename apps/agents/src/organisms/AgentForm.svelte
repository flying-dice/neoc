<!-- @organism AgentForm — the form an agent is added or changed in: its name, folder and picture, and what its type takes (a remote's address and token; a program's or a container's command and environment; a container's image, skills folder and skills), its MCP servers, with the way to remove it -->
<script lang="ts">
  type Secret = VosApp.Agents.Secret
  type SkillSummary = VosApp.Agents.SkillSummary
  import Avatar from '../atoms/Avatar.svelte'
  import VariableRow from '../atoms/VariableRow.svelte'
  import ServerSection from '../molecules/ServerSection.svelte'
  import type { AgentForm, Choices } from '../lib/agents.svelte'
  import { PICTURE_TYPES, pictureOf } from '../lib/avatar'
  import { blankServer, blankVariable } from '../lib/form'
  import { KINDS, folderFor, folderHint, takes } from '../lib/kinds'

  interface Props {
    form: AgentForm
    /** Why the backend refused the form, shown under the fields. */
    failure?: string | null
    savable: boolean
    /** The Keychain's items a value can be, or why they could not be read; values can still be written out. */
    secrets?: Choices<Secret>
    /** The Skills app's skills a container agent can use, or why there are none. */
    skills?: Choices<SkillSummary>
    /** The images Docker has, for a container agent's image, or why there are none; a name can still be typed. */
    images?: Choices<string>
    onsave: () => void
    oncancel: () => void
    onremove: () => void
  }

  const NONE = { items: [], problem: null }
  let { form = $bindable(), failure = null, savable, secrets = NONE, skills = NONE, images = NONE, onsave, oncancel, onremove }: Props = $props()

  const valueOf = (e: Event): string => (e.currentTarget as unknown as { value: string }).value
  const adding = $derived(form.agent === null)
  const hasToken = $derived(form.agent?.token === true)
  const needs = $derived(takes(form.kind))
  const kind = $derived(KINDS.find((k) => k.kind === form.kind)!)
  // a skill the agent uses stays a choice, though the Skills app no longer lists it (or could not be read)
  const skillChoices = $derived([
    ...skills.items,
    ...form.skills.filter((name) => !skills.items.some((s) => s.name === name)).map((name) => ({ name, description: 'Not in the Skills app now.' }))
  ])
  const imageChoices = $derived([{ value: '', label: 'Choose…' }, ...images.items.map((image) => ({ value: image, label: image }))])

  function pick(skill: string, on: boolean): void {
    form.skills = on ? [...form.skills, skill] : form.skills.filter((s) => s !== skill)
  }

  let picker = $state<HTMLInputElement>()
  // the file chosen last is not a picture the browser can read
  let unreadable = $state(false)

  /** Make the picture kept from the file chosen: a square from its middle, small. */
  async function choose(e: Event): Promise<void> {
    const input = e.currentTarget as HTMLInputElement
    const file = input.files?.[0]
    // the same file can be chosen again
    input.value = ''
    if (!file) return
    try {
      form.avatar = await pictureOf(file)
      unreadable = false
    } catch (e) {
      vos.tracing.warn('could not read the chosen picture', { error: String(e), type: file.type })
      unreadable = true
    }
  }
</script>

<form
  class="form"
  aria-label={adding ? 'Add an agent' : 'Change the agent'}
  data-testid="agents-form"
  onsubmit={(e) => {
    e.preventDefault()
    onsave()
  }}
>
  <header class="bar">
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="plain" hit onclick={oncancel} data-testid="agents-form-cancel">Cancel</ui-button>
    <h1>{adding ? 'New Agent' : 'Agent'}</h1>
    <ui-button type="submit" variant="plain" hit disabled={!savable} data-testid="agents-form-save">{adding ? 'Add' : 'Save'}</ui-button>
  </header>
  <div class="fields">
    <ui-list variant="inset" label="The agent">
      <ui-section
        header="Agent"
        footer={failure ??
          (needs.address
            ? `The address is where the agent speaks the Agent Client Protocol over a WebSocket (acpd serves one at /acp). ${folderHint(form.kind)}`
            : form.kind === 'program'
              ? `The command speaks the Agent Client Protocol on its standard input and output. ${folderHint(form.kind)}`
              : folderHint(form.kind))}
        footer-tone={failure ? 'error' : 'note'}
        data-testid="agents-form-agent"
      >
        <ui-row heading="Type" value={kind.label} icon={kind.icon} icon-color={kind.tint} data-testid="agents-form-kind"></ui-row>
        <ui-row>
          <span class="key">Name</span>
          <ui-text-field
            label="Name"
            placeholder="Name"
            maxlength={80}
            autocomplete="off"
            variant="bare"
            value={form.name}
            oninput={(e: Event) => (form.name = valueOf(e))}
            data-testid="agents-form-name"
          ></ui-text-field>
        </ui-row>
        {#if needs.address}
          <ui-row>
            <span class="key">Address</span>
            <ui-text-field
              label="Address"
              placeholder="ws://example.com:5424/acp"
              type="url"
              inputmode="url"
              autocapitalize="off"
              autocomplete="off"
              maxlength={2000}
              variant="bare"
              value={form.url}
              oninput={(e: Event) => (form.url = valueOf(e))}
              data-testid="agents-form-url"
            ></ui-text-field>
          </ui-row>
        {/if}
        {#if form.kind === 'program'}
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
              data-testid="agents-form-command"
            ></ui-text-field>
          </ui-row>
        {/if}
        <ui-row>
          <span class="key">Folder</span>
          <ui-text-field
            class={needs.address ? undefined : 'mono'}
            label="Folder"
            placeholder={folderFor(form.kind) || '/home/me/project'}
            autocapitalize="off"
            autocomplete="off"
            maxlength={4000}
            variant="bare"
            value={form.folder}
            oninput={(e: Event) => (form.folder = valueOf(e))}
            data-testid="agents-form-folder"
          ></ui-text-field>
        </ui-row>
      </ui-section>

      {#if needs.image}
        <ui-section
          header="Container"
          footer={images.problem ??
            'Each chat gets a container of its own, made from the image: build or pull it in Containers. The command is the container’s program, and speaks the Agent Client Protocol on its standard input and output.'}
          footer-tone={images.problem ? 'error' : 'note'}
          data-testid="agents-form-container"
        >
          <ui-row>
            <span class="key">Image</span>
            <ui-text-field
              class="mono"
              label="Image"
              placeholder="agent-coder:1"
              autocapitalize="off"
              autocomplete="off"
              maxlength={500}
              variant="bare"
              value={form.image}
              oninput={(e: Event) => (form.image = valueOf(e))}
              data-testid="agents-form-image"
            ></ui-text-field>
            {#if images.items.length > 0}
              <ui-picker
                slot="trailing"
                label="Choose one of Docker's images"
                variant="bare"
                options={imageChoices}
                value={images.items.includes(form.image) ? form.image : ''}
                onchange={(e) => (form.image = e.detail.value || form.image)}
                data-testid="agents-form-image-choice"
              ></ui-picker>
            {/if}
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
              data-testid="agents-form-command"
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
              data-testid="agents-form-skills-dir"
            ></ui-text-field>
          </ui-row>
        </ui-section>
      {/if}

      {#if needs.address}
        <ui-section
          header="Token"
          footer="Sent to the address as a bearer token when this app connects. It is kept in this machine's keychain, and is never shown again."
        >
          <ui-row>
            <span class="key">Token</span>
            <ui-text-field
              label="Token"
              placeholder={hasToken ? 'Has a token: write a new one to replace it' : 'None'}
              type="password"
              autocapitalize="off"
              autocomplete="off"
              maxlength={4000}
              variant="bare"
              disabled={form.forget}
              value={form.token}
              oninput={(e: Event) => (form.token = valueOf(e))}
              data-testid="agents-form-token"
            ></ui-text-field>
          </ui-row>
          {#if hasToken}
            <ui-row heading="Remove the token">
              <ui-toggle
                slot="trailing"
                label="Remove the token"
                checked={form.forget}
                onchange={(e: Event) => (form.forget = (e.currentTarget as unknown as { checked: boolean }).checked)}
                data-testid="agents-form-forget"
              ></ui-toggle>
            </ui-row>
          {/if}
        </ui-section>
      {/if}

      {#if needs.command}
        <ui-section
          header="Environment"
          footer={secrets.problem ??
            `A value can be a Keychain item's password, read when ${form.kind === 'container' ? "a chat's container is made" : 'the program starts'}.`}
          footer-tone={secrets.problem ? 'error' : 'note'}
          data-testid="agents-form-env"
        >
          {#each form.env as variable, i (variable.key)}
            <VariableRow
              bind:variable={form.env[i]!}
              kind="Variable"
              secrets={secrets.items}
              testid="agents-form-env-{i}"
              onremove={() => (form.env = form.env.filter((v) => v.key !== variable.key))}
            />
          {/each}
          <ui-row heading="Add a variable" action tappable onpress={() => form.env.push(blankVariable())} data-testid="agents-form-add-env"></ui-row>
        </ui-section>
      {/if}

      {#each form.mcp as server, i (server.key)}
        <ServerSection
          bind:server={form.mcp[i]!}
          secrets={secrets.items}
          kind={form.kind}
          testid="agents-form-server-{i}"
          onremove={() => (form.mcp = form.mcp.filter((s) => s.key !== server.key))}
        />
      {/each}
      <ui-section header={form.mcp.length === 0 ? 'MCP servers' : undefined} footer="Given to its sessions, besides this machine's own tools.">
        <ui-row heading="Add a program server" action tappable onpress={() => form.mcp.push(blankServer('stdio'))} data-testid="agents-form-add-stdio"></ui-row>
        <ui-row heading="Add a URL server" action tappable onpress={() => form.mcp.push(blankServer('http'))} data-testid="agents-form-add-http"></ui-row>
      </ui-section>

      {#if needs.image}
        <ui-section
          header="Skills"
          footer={skills.problem ?? "The Skills app's skills, copied into each new chat's container, in the skills folder."}
          footer-tone={skills.problem ? 'error' : 'note'}
          data-testid="agents-form-skills"
        >
          {#if skillChoices.length === 0}
            <ui-row heading="The Skills app has no skills."></ui-row>
          {/if}
          {#each skillChoices as skill (skill.name)}
            <ui-row heading={skill.name} subheading={skill.description || undefined}>
              <ui-check
                slot="trailing"
                label={skill.name}
                checked={form.skills.includes(skill.name)}
                onchange={(e) => pick(skill.name, e.detail.checked)}
                data-testid="agents-form-skill-{skill.name}"
              ></ui-check>
            </ui-row>
          {/each}
        </ui-section>
      {/if}

      <ui-section
        header="Picture"
        footer={unreadable
          ? 'That file is not a picture this app can read. Choose a PNG, JPEG, WebP or GIF image.'
          : "A square from the middle of the picture is kept. Without one, the agent's initials are shown."}
        footer-tone={unreadable ? 'error' : 'note'}
        data-testid="agents-form-picture"
      >
        <!-- no space between what goes in the row's slots: a space would be its content, in place of its heading -->
        <ui-row heading={form.avatar === '' ? 'Its initials' : 'Its picture'}>
          <span slot="leading" class="preview">
            <Avatar id={form.agent?.id ?? ''} name={form.name.trim() || 'New Agent'} picture={form.avatar} size={44} />
          </span><span slot="trailing" class="actions">
            {#if form.avatar !== ''}
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
              <ui-button variant="plain" size="small" destructive hit onclick={() => (form.avatar = '')} data-testid="agents-form-remove-picture">Remove</ui-button>
            {/if}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <ui-button variant="plain" size="small" hit onclick={() => picker?.click()} data-testid="agents-form-choose-picture">
              {form.avatar === '' ? 'Choose…' : 'Change…'}
            </ui-button>
          </span>
        </ui-row>
      </ui-section>
      {#if !adding}
        <ui-section footer={form.kind === 'container' ? 'Its chats, with their containers and volumes, go with it. Its image stays, in Containers.' : 'Its chats stay, and can be read.'}>
          <ui-row heading="Remove this agent" destructive tappable onpress={onremove} data-testid="agents-form-remove"></ui-row>
        </ui-section>
      {/if}
    </ui-list>
    <!-- the picker the Choose button opens; what it gives is drawn small in the page, never sent as it is -->
    <input bind:this={picker} type="file" accept={PICTURE_TYPES} hidden onchange={choose} data-testid="agents-form-picture-file" />
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
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    gap: 8px;
    padding: 0 12px 4px;
    border-bottom: 1px solid var(--separator);
  }
  .bar > :last-child {
    justify-self: end;
  }
  h1 {
    margin: 0;
    font: var(--type-headline);
  }
  .fields {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 16px 12px;
  }
  ui-row {
    --row-gap: 12px;
  }
  .key {
    flex: none;
    width: 6.5em;
  }
  ui-text-field {
    flex: 1;
    min-width: 0;
  }
  .mono {
    font-family: var(--font-mono);
  }
  .preview {
    display: flex;
    padding: 4px 0;
  }
  .actions {
    display: flex;
    gap: 4px;
  }
</style>
