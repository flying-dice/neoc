type Building = VosApp.Containers.BuildingParams
type BuildStatus = VosApp.Containers.BuildStatus
type Image = VosApp.Containers.Image
type Recipe = VosApp.Containers.Recipe
import { shortId } from './format'

/**
 * What the window needs of the backend for the image recipes: the list, each way of changing it, and the build (the
 * app's private methods on the bus, `_recipes` ...: `App.svelte` sends them). Every call answers with the recipes as
 * they now stand.
 */
export interface RecipesRpc {
  recipes(): Promise<Recipe[]>
  addRecipe(params: VosApp.Containers.AddRecipeParams): Promise<Recipe[]>
  updateRecipe(params: VosApp.Containers.UpdateRecipeParams): Promise<Recipe[]>
  removeRecipe(params: VosApp.Containers.RemoveRecipeParams): Promise<Recipe[]>
  build(params: VosApp.Containers.BuildParams): Promise<Recipe[]>
}

/** How the last build of a recipe that this window heard of goes: its lines so far, and whether it is over. */
export interface Build {
  lines: string[]
  status: BuildStatus
}

/** A recipe as the form holds it while it is filled in. */
export interface RecipeDraft {
  /** The name the image is built as: `tools:1`. */
  tag: string
  dockerfile: string
}

/** The form a recipe is added or changed in, while it is open. */
export interface RecipeForm {
  /** The id of the recipe it changes; null for a new one, until it is first saved. */
  recipe: string | null
  draft: RecipeDraft
  /** Why the backend refused it, in its words. */
  failure: string | null
  saving: boolean
}

/** How a recipe's image stands, in a word for its colour and a line for the person. */
export interface RecipeStatus {
  tone: 'running' | 'done' | 'failed' | 'none'
  text: string
}

const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))

export const blankRecipe = (): RecipeDraft => ({ tag: '', dockerfile: '' })

/**
 * How a recipe's image stands: being built (this window started it, or the backend says so), the last build failed, it
 * was never built, the image it built is gone (removed, or another Docker is chosen), the recipe changed since, or it
 * is built from the recipe as it is. `null` is a recipe not saved yet.
 */
export function statusOf(recipe: Recipe | null, build: Build | null, images: readonly Image[]): RecipeStatus {
  if (build?.status === 'running' || recipe?.building) return { tone: 'running', text: 'Building…' }
  if (recipe === null) return { tone: 'none', text: 'Not built yet.' }
  if (recipe.problem !== '') return { tone: 'failed', text: `Failed: ${recipe.problem}` }
  if (recipe.image === '') return { tone: 'none', text: 'Not built yet.' }
  if (!images.some((i) => i.id === recipe.image)) return { tone: 'none', text: 'The image it built is gone: build it again.' }
  if (!recipe.current) return { tone: 'none', text: `Changed since the last build (${shortId(recipe.image)}).` }
  return { tone: 'done', text: `Built: ${shortId(recipe.image)}` }
}

/**
 * One window's image recipes: the backend's, read again when Docker changes and when a build is over, and what is the
 * window's own while they are changed (the form, the builds it heard of). A failure that belongs to no form is said
 * where the window says the rest (`fail`); once a build is over, the images are read again (`built`).
 */
export class Recipes {
  #recipes = $state.raw<readonly Recipe[]>([])
  #builds = $state.raw<Readonly<Record<string, Build>>>({})
  /** The form a recipe is added or changed in, while it is open. */
  form = $state<RecipeForm | null>(null)

  readonly #rpc: RecipesRpc
  readonly #fail: (message: string) => void
  readonly #built: () => Promise<void>

  constructor(rpc: RecipesRpc, fail: (message: string) => void, built: () => Promise<void>) {
    this.#rpc = rpc
    this.#fail = fail
    this.#built = built
  }

  get all(): readonly Recipe[] {
    return this.#recipes
  }
  /** The recipe the form changes, as the backend has it now; null for a new one. */
  get editing(): Recipe | null {
    const id = this.form?.recipe
    return (id && this.#recipes.find((r) => r.id === id)) || null
  }
  /** The last build of a recipe this window heard of; null for none. */
  buildOf(recipe: string | null): Build | null {
    return (recipe && this.#builds[recipe]) || null
  }
  /** A build of the recipe is under way, as this window heard or the backend says. */
  building(recipe: Recipe | null): boolean {
    return recipe !== null && (recipe.building || this.buildOf(recipe.id)?.status === 'running')
  }
  /** The form can be saved: it names an image and has a Dockerfile, and is not being saved. */
  get savable(): boolean {
    const form = this.form
    return form !== null && !form.saving && form.draft.tag.trim() !== '' && form.draft.dockerfile.trim() !== ''
  }

  /** Read the recipes again. A failure is said; what was there stays. */
  async load(): Promise<void> {
    try {
      this.#recipes = await this.#rpc.recipes()
    } catch (e) {
      this.#fail(messageOf(e))
    }
  }

  /** A line of a build: a build that was over is followed by a new one. Once it is over, the recipes and images are read again. */
  hear({ recipe, line, status }: Building): void {
    const before = this.#builds[recipe]
    const lines = before?.status === 'running' ? before.lines : []
    this.#builds = { ...this.#builds, [recipe]: { lines: [...lines, line], status } }
    if (status !== 'running') void Promise.all([this.load(), this.#built()])
  }

  /** Open the form for a new recipe. */
  compose(): void {
    this.form = { recipe: null, draft: blankRecipe(), failure: null, saving: false }
  }

  /** Open the form on a recipe, to change, build or remove it. */
  edit(recipe: Recipe): void {
    this.form = { recipe: recipe.id, draft: { tag: recipe.tag, dockerfile: recipe.dockerfile }, failure: null, saving: false }
  }

  /** Leave the form, unless it is being saved. */
  close(): void {
    if (this.form?.saving) return
    this.form = null
  }

  /** Run a call of the form's; answers whether it worked. A refusal is kept on the form. */
  async #run(form: RecipeForm, call: () => Promise<void>): Promise<boolean> {
    form.saving = true
    form.failure = null
    try {
      await call()
      return true
    } catch (e) {
      form.failure = messageOf(e)
      return false
    } finally {
      form.saving = false
    }
  }

  /** Send the form; answers with the recipe as saved, or null when the backend refused it (the reason is kept). */
  async #save(): Promise<Recipe | null> {
    const form = this.form
    if (form === null || !this.savable) return null
    const { tag, dockerfile } = form.draft
    let saved: Recipe | null = null
    await this.#run(form, async () => {
      const before = new Set(this.#recipes.map((r) => r.id))
      const id = form.recipe
      this.#recipes = id === null ? await this.#rpc.addRecipe({ tag, dockerfile }) : await this.#rpc.updateRecipe({ recipe: id, tag, dockerfile })
      saved = this.#recipes.find((r) => (id === null ? !before.has(r.id) : r.id === id)) ?? null
    })
    return saved
  }

  /** Save the form, and close it. It stays open, with the reason, when the backend refuses it. */
  async save(): Promise<void> {
    const form = this.form
    if ((await this.#save()) && this.form === form) this.form = null
  }

  /** Save the form, which stays open on the recipe as saved, and build its image: the log fills as the build goes. */
  async build(): Promise<void> {
    const form = this.form
    const saved = await this.#save()
    if (!saved || form === null || this.form !== form) return
    form.recipe = saved.id
    const id = saved.id
    this.#builds = { ...this.#builds, [id]: { lines: [], status: 'running' } }
    const started = await this.#run(form, async () => void (this.#recipes = await this.#rpc.build({ recipe: id })))
    if (!started) {
      this.#builds = { ...this.#builds, [id]: { lines: [], status: 'failed' } }
      return
    }
    // a build that was over before its start was answered: the answer said it was under way
    if (this.#builds[id]?.status !== 'running') await this.load()
  }

  /** Remove the recipe the form changes; its images stay. The form stays open, with the reason, when it is refused. */
  async remove(): Promise<void> {
    const form = this.form
    const id = form?.recipe
    if (!form || !id) return
    if (await this.#run(form, async () => void (this.#recipes = await this.#rpc.removeRecipe({ recipe: id })))) {
      if (this.form === form) this.form = null
    }
  }
}
