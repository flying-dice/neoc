<!-- @organism ImageList — the images Docker has, each with its id, size, the day it was built and how many containers were made from it, the buttons that remove one or make a container of it, the field an image is pulled by with how the pull goes, the recipes images are built from with how each one's image stands, and the prune of the dangling ones with what it freed -->
<script lang="ts">
  type Image = VosApp.Containers.Image
  type Recipe = VosApp.Containers.Recipe
  import PullMeter from '../molecules/PullMeter.svelte'
  import ResourceRow from '../molecules/ResourceRow.svelte'
  import SectionBar from '../molecules/SectionBar.svelte'
  import { bytes, count, day, imageName, listed, shortId, usage } from '../lib/format'
  import type { RecipeStatus } from '../lib/recipes.svelte'
  import { danglingOf, type Pull } from '../lib/resources.svelte'

  interface Props {
    images: readonly Image[]
    /** The pull by name: what is typed goes into it. */
    pull: Pull
    /** The dangling images are being pruned. */
    pruning?: boolean
    /** What the last prune took, in words. */
    report?: string
    /** Show the way back to the sidebar (a window too narrow for both). */
    back?: boolean
    onback?: () => void
    onpull: () => void
    onremove: (image: Image) => void
    /** Make a container of the image: the form, with the image filled in. */
    onnew: (image: Image) => void
    onprune: () => void
    /** The recipes images are built from. */
    recipes: readonly Recipe[]
    /** How a recipe's image stands. */
    statusOf: (recipe: Recipe) => RecipeStatus
    /** Open a recipe, to change, build or remove it. */
    onrecipe: (recipe: Recipe) => void
    /** Open the form for a new recipe. */
    onnewrecipe: () => void
  }

  let { images, pull = $bindable(), pruning = false, report, back = false, onback, onpull, onremove, onnew, onprune, recipes, statusOf, onrecipe, onnewrecipe }: Props = $props()

  const valueOf = (e: Event): string => (e.currentTarget as unknown as { value: string }).value
  const total = $derived(images.reduce((sum, image) => sum + image.size, 0))
  const dangling = $derived(danglingOf(images))
  // a prune removes images: the person is asked first
  let asking = $state(false)
</script>

<section class="list" aria-label="Images">
  <SectionBar title="Images" summary={images.length === 0 ? 'No images' : `${count(images.length, 'image')} · ${bytes(total)}`} {back} {onback}>
    {#if pruning}<ui-spinner label="Removing the dangling images" data-testid="containers-images-pruning"></ui-spinner>{/if}
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button
      variant="plain"
      square
      hit
      destructive
      disabled={pruning || dangling.length === 0}
      label="Remove the dangling images"
      onclick={() => (asking = true)}
      data-testid="containers-images-prune"
    >
      <ui-icon slot="icon" name="trash"></ui-icon>
    </ui-button>
  </SectionBar>
  {#if asking}
    <ui-alert
      open
      destructive
      action-label="Remove"
      heading="Remove {count(dangling.length, 'dangling image')}?"
      message="The images that have no name and that no container was made from go: {bytes(dangling.reduce((sum, image) => sum + image.size, 0))}. A newer pull of their name left them behind."
      data-testid="containers-images-prune-confirm"
      oncancel={() => (asking = false)}
      onconfirm={() => {
        asking = false
        onprune()
      }}
    ></ui-alert>
  {/if}
  <div class="scroll">
    <form
      class="pull-form"
      aria-label="Pull an image"
      onsubmit={(e) => {
        e.preventDefault()
        onpull()
      }}
    >
      <ui-list variant="inset" label="Pull an image">
        <ui-section
          header="Pull an Image"
          footer={pull.failure ?? 'By its name, from its registry: a newer one of a name Docker has takes the name.'}
          footer-tone={pull.failure ? 'error' : 'note'}
          data-testid="containers-pull-about"
        >
          <ui-row>
            <ui-text-field
              class="reference"
              label="Image"
              placeholder="nginx:1.27"
              autocapitalize="off"
              autocomplete="off"
              maxlength={512}
              variant="bare"
              disabled={pull.busy}
              value={pull.reference}
              oninput={(e: Event) => (pull.reference = valueOf(e))}
              data-testid="containers-pull-image"
            ></ui-text-field>
            <span slot="trailing" class="go">
              {#if pull.busy}<ui-spinner label="Pulling {pull.reference}" data-testid="containers-pull-busy"></ui-spinner>{/if}
              <ui-button type="submit" variant="plain" hit disabled={pull.busy || pull.reference.trim() === ''} data-testid="containers-pull">Pull</ui-button>
            </span>
          </ui-row>
        </ui-section>
      </ui-list>
    </form>
    {#if pull.busy && pull.progress}
      <PullMeter pulling={pull.progress} testid="containers-pull-progress" />
    {/if}
    <ui-list variant="inset" label="Build an image">
      <ui-section
        header="Build an Image"
        footer="A recipe is the name of an image and the Dockerfile it is built from."
        data-testid="containers-recipes"
      >
        {#each recipes as recipe (recipe.id)}
          <ui-row
            heading={recipe.tag}
            subheading={statusOf(recipe).text}
            icon="doc"
            chevron
            tappable
            onpress={() => onrecipe(recipe)}
            data-testid="containers-recipe-{recipe.tag}"
          ></ui-row>
        {/each}
        <ui-row heading="New Recipe" action tappable onpress={onnewrecipe} data-testid="containers-recipe-new"></ui-row>
      </ui-section>
    </ui-list>
    {#if report}
      <ui-banner tone="success" data-testid="containers-images-report">{report}</ui-banner>
    {/if}
    {#if images.length > 0}
      <ul class="items" data-testid="containers-images">
        {#each images as image (image.id)}
          {@const name = imageName(image)}
          <ResourceRow
            {name}
            lines={[
              `${shortId(image.id)} · ${bytes(image.size)} · ${day(new Date(image.created * 1000))}`,
              ...(image.tags.length > 1 ? [`also ${listed(image.tags.slice(1))}`] : [])
            ]}
            note={image.containers.length === 0 ? usage([]) : `${usage(image.containers)}: ${listed(image.containers)}`}
            unused={image.containers.length === 0}
            testid="containers-image-{image.tags[0] ?? shortId(image.id)}"
            actions={[
              // a dangling image has no name to make a container of
              ...(image.tags.length > 0
                ? [{ label: `New container from ${name}`, icon: 'plus', testid: `containers-image-${name}-new`, onclick: () => onnew(image) }]
                : []),
              { label: `Remove ${name}`, icon: 'trash', destructive: true, testid: `containers-image-${image.tags[0] ?? shortId(image.id)}-remove`, onclick: () => onremove(image) }
            ]}
          />
        {/each}
      </ul>
    {:else}
      <ui-empty-state
        icon="doc"
        heading="No Images"
        message="Docker has none. Pull one by its name above, or make a container: its image is pulled for it."
        data-testid="containers-images-empty"
      ></ui-empty-state>
    {/if}
  </div>
</section>

<style>
  .list {
    box-sizing: border-box;
    min-width: 0;
    height: 100%;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
    display: flex;
    flex-direction: column;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 12px 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .pull-form {
    margin: 0;
  }
  .reference {
    flex: 1;
    min-width: 0;
    font-family: var(--font-mono);
  }
  .go {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .items {
    margin: 0;
    padding: 0;
  }
</style>
