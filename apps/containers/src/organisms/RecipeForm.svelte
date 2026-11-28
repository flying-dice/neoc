<!-- @organism RecipeForm — the form an image recipe is added or changed in: the name its image is built as and its Dockerfile, how its image stands with the log of its build, the Build that saves and builds it, the way to remove it after a second look, and why the backend refused it -->
<script lang="ts">
  type Recipe = VosApp.Containers.Recipe
  import BuildLog from '../molecules/BuildLog.svelte'
  import type { RecipeForm, RecipeStatus } from '../lib/recipes.svelte'

  interface Props {
    form: RecipeForm
    /** The recipe the form changes, as the backend has it now; null for a new one. */
    recipe: Recipe | null
    /** How its image stands. */
    status: RecipeStatus
    /** The lines of the last build of it this window heard of. */
    lines: readonly string[]
    savable: boolean
    onsave: () => void
    onbuild: () => void
    oncancel: () => void
    onremove: () => void
  }

  let { form = $bindable(), recipe, status, lines, savable, onsave, onbuild, oncancel, onremove }: Props = $props()

  const valueOf = (e: Event): string => (e.currentTarget as unknown as { value: string }).value
  const saving = $derived(form.saving)
  const building = $derived(status.tone === 'running')
  // removing asks first
  let asking = $state(false)
</script>

<form
  class="form"
  aria-label={recipe ? `Change the recipe of ${recipe.tag}` : 'New Recipe'}
  data-testid="containers-recipe-form"
  onsubmit={(e) => {
    e.preventDefault()
    onsave()
  }}
>
  <header class="bar">
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="plain" hit disabled={saving} onclick={oncancel} data-testid="containers-recipe-form-cancel">Cancel</ui-button>
    <h1 data-testid="containers-recipe-form-title">{recipe ? recipe.tag : 'New Recipe'}</h1>
    <span class="save">
      {#if saving}<ui-spinner label="Saving" data-testid="containers-recipe-form-saving"></ui-spinner>{/if}
      <ui-button type="submit" variant="plain" hit disabled={!savable} data-testid="containers-recipe-form-save">{recipe ? 'Save' : 'Add'}</ui-button>
    </span>
  </header>
  <div class="fields">
    {#if form.failure}
      <ui-banner tone="error" data-testid="containers-recipe-form-failure">{form.failure}</ui-banner>
    {/if}
    <ui-list variant="inset" label="The recipe">
      <ui-section header="Image" footer="The name the image is built as, with its tag. Building again moves the name to the new image.">
        <ui-row>
          <span class="key">Name</span>
          <ui-text-field
            class="mono"
            label="Image name"
            placeholder="tools:1"
            autocapitalize="off"
            autocomplete="off"
            maxlength={512}
            variant="bare"
            disabled={saving}
            value={form.draft.tag}
            oninput={(e: Event) => (form.draft.tag = valueOf(e))}
            data-testid="containers-recipe-form-tag"
          ></ui-text-field>
        </ui-row>
      </ui-section>
      <ui-section header="Dockerfile" footer="The whole build context: what the image needs, it downloads.">
        <ui-row>
          <ui-text-area
            class="mono"
            label="Dockerfile"
            placeholder={'FROM alpine:3.20\nRUN apk add --no-cache curl git'}
            rows={10}
            maxlength={100000}
            autocapitalize="off"
            variant="bare"
            disabled={saving}
            value={form.draft.dockerfile}
            oninput={(e: Event) => (form.draft.dockerfile = valueOf(e))}
            data-testid="containers-recipe-form-dockerfile"
          ></ui-text-area>
        </ui-row>
      </ui-section>
      <ui-section header="Build" footer="Build saves the recipe, then builds its image from the Dockerfile." data-testid="containers-recipe-form-build-about">
        <ui-row>
          <BuildLog {status} {lines} testid="containers-recipe-log" />
        </ui-row>
        <ui-row
          heading={building ? 'Building…' : 'Build'}
          action
          tappable
          disabled={building || !savable}
          onpress={onbuild}
          data-testid="containers-recipe-form-build"
        ></ui-row>
      </ui-section>
      {#if recipe}
        <ui-section footer="The images it built stay: remove them in the list of images.">
          <ui-row heading="Remove Recipe" destructive tappable disabled={saving} onpress={() => (asking = true)} data-testid="containers-recipe-form-remove"></ui-row>
        </ui-section>
      {/if}
    </ui-list>
  </div>
  {#if asking && recipe}
    <ui-alert
      open
      destructive
      action-label="Remove"
      heading="Remove the recipe of {recipe.tag}?"
      message="Its name and Dockerfile go. The images it built stay."
      data-testid="containers-recipe-form-remove-confirm"
      oncancel={() => (asking = false)}
      onconfirm={() => {
        asking = false
        onremove()
      }}
    ></ui-alert>
  {/if}
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
    grid-template-columns: minmax(max-content, 1fr) minmax(0, 2fr) minmax(max-content, 1fr);
    align-items: center;
    gap: 8px;
    padding: 0 8px 8px;
    border-bottom: 1px solid var(--separator);
  }
  h1 {
    min-width: 0;
    margin: 0;
    text-align: center;
    font: var(--type-headline);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .bar > ui-button {
    justify-self: start;
  }
  .save {
    justify-self: end;
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .fields {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 16px 12px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  ui-row {
    --row-gap: 12px;
  }
  .key {
    flex: none;
    width: 5em;
  }
  ui-text-field,
  ui-text-area {
    flex: 1;
    min-width: 0;
  }
  .mono {
    font-family: var(--font-mono);
  }
</style>
