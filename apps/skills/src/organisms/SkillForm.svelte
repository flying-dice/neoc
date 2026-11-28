<!-- @organism SkillForm — the form a skill is added or changed in: its name, its SKILL.md and its other files, with the way to remove it -->
<script lang="ts">
  import { blankFile, type SkillDraft } from '../lib/form'

  interface Props {
    form: SkillDraft
    /** Why the backend refused the form, shown over the fields. */
    failure?: string | null
    savable: boolean
    onsave: () => void
    oncancel: () => void
    onremove: () => void
  }

  let { form = $bindable(), failure = null, savable, onsave, oncancel, onremove }: Props = $props()

  const valueOf = (e: Event): string => (e.currentTarget as unknown as { value: string }).value
  const adding = $derived(form.skill === null)
</script>

<form
  class="form"
  aria-label={adding ? 'Add a skill' : 'Change the skill'}
  data-testid="skills-form"
  onsubmit={(e) => {
    e.preventDefault()
    onsave()
  }}
>
  <header class="bar">
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="plain" hit onclick={oncancel} data-testid="skills-form-cancel">Cancel</ui-button>
    <h1>{adding ? 'New Skill' : form.skill?.name}</h1>
    <ui-button type="submit" variant="plain" hit disabled={!savable} data-testid="skills-form-save">{adding ? 'Add' : 'Save'}</ui-button>
  </header>
  <div class="fields">
    {#if failure}
      <ui-banner tone="error" data-testid="skills-form-failure">{failure}</ui-banner>
    {/if}
    <ui-list variant="inset" label="The skill">
      <ui-section header="Skill" footer="The name of its folder where an agent is given it: letters, digits, ., _ and -.">
        <ui-row>
          <span class="key">Name</span>
          <ui-text-field
            class="mono"
            label="Name"
            placeholder="review"
            maxlength={64}
            autocapitalize="off"
            autocomplete="off"
            variant="bare"
            value={form.name}
            oninput={(e: Event) => (form.name = valueOf(e))}
            data-testid="skills-form-name"
          ></ui-text-field>
        </ui-row>
      </ui-section>
      <ui-section header="SKILL.md">
        <ui-row>
          <ui-text-area
            label="SKILL.md"
            placeholder={'---\nname: review\ndescription: When to use it.\n---'}
            rows={10}
            maxlength={200000}
            variant="bare"
            value={form.text}
            oninput={(e: Event) => (form.text = valueOf(e))}
            data-testid="skills-form-text"
          ></ui-text-area>
        </ui-row>
      </ui-section>
      {#each form.files as file, i (file.key)}
        <ui-section header="File" data-testid="skills-form-file-{i}">
          <ui-row>
            <span class="key">Path</span>
            <ui-text-field
              class="mono"
              label="File path"
              placeholder="scripts/check.sh"
              maxlength={500}
              autocapitalize="off"
              autocomplete="off"
              variant="bare"
              value={file.path}
              oninput={(e: Event) => (form.files[i]!.path = valueOf(e))}
              data-testid="skills-form-file-{i}-path"
            ></ui-text-field>
          </ui-row>
          <ui-row>
            <ui-text-area
              label="File text"
              rows={6}
              maxlength={200000}
              variant="bare"
              value={file.text}
              oninput={(e: Event) => (form.files[i]!.text = valueOf(e))}
              data-testid="skills-form-file-{i}-text"
            ></ui-text-area>
          </ui-row>
          <ui-row
            heading="Remove this file"
            destructive
            tappable
            onpress={() => (form.files = form.files.filter((f) => f.key !== file.key))}
            data-testid="skills-form-file-{i}-remove"
          ></ui-row>
        </ui-section>
      {/each}
      <ui-section footer="Other files of the skill, by their path in its folder.">
        <ui-row heading="Add a file" action tappable onpress={() => form.files.push(blankFile())} data-testid="skills-form-add-file"></ui-row>
      </ui-section>
      {#if !adding}
        <ui-section footer="An agent given it before keeps its copy.">
          <ui-row heading="Remove this skill" destructive tappable onpress={onremove} data-testid="skills-form-remove"></ui-row>
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
  }
  .bar {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    gap: 8px;
    padding: 0 8px 8px;
    border-bottom: 1px solid var(--separator);
  }
  .bar > ui-button:first-child {
    justify-self: start;
  }
  .bar > ui-button:last-child {
    justify-self: end;
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
    --list-inset: 16px;
    padding: 16px 0;
  }
  ui-banner {
    display: block;
    margin: 0 16px 12px;
  }
  ui-row {
    --row-gap: 12px;
  }
  .key {
    flex: none;
    width: 5.5em;
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
    font: var(--type-footnote);
    font-family: var(--font-mono);
  }
</style>
