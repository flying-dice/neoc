<!-- @organism SkillLibrary — the skills a copilot can be given: each by its name and how many files it has, a tap opens it to change, and the way to add one -->
<script lang="ts">
  type Skill = VosApp.Copilot.Skill

  interface Props {
    skills: readonly Skill[]
    /** Show the way back to the list (a window too narrow for both). */
    back?: boolean
    /** Open the skill form: to change the skill given, or to add one (null). */
    onedit: (skill: Skill | null) => void
    onback?: () => void
  }

  let { skills, back = false, onedit, onback }: Props = $props()

  const filesOf = (skill: Skill): string => {
    const n = skill.files.length + 1
    return `SKILL.md${n > 1 ? ` and ${n - 1} ${n === 2 ? 'file' : 'files'}` : ''}`
  }
</script>

<section class="library" aria-label="Skills" data-testid="copilot-skills">
  <header class="bar">
    {#if back}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit label="Back to the copilots" onclick={onback} data-testid="copilot-skills-back">
        <ui-icon slot="icon" name="chevron.left"></ui-icon>
        Copilots
      </ui-button>
    {/if}
    <h1>Skills</h1>
  </header>
  <div class="fields">
    <ui-list variant="inset" label="Skills">
      <ui-section footer="A skill is a SKILL.md and its files. A copilot that uses it gets a copy in each new chat's container.">
        {#each skills as skill (skill.id)}
          <ui-row heading={skill.name} subheading={filesOf(skill)} tappable chevron onpress={() => onedit(skill)} data-testid="copilot-skill-{skill.id}"></ui-row>
        {/each}
        <ui-row heading="Add a skill" action tappable onpress={() => onedit(null)} data-testid="copilot-add-skill"></ui-row>
      </ui-section>
    </ui-list>
  </div>
</section>

<style>
  .library {
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
    gap: 8px;
    min-height: 34px;
    padding: 0 8px 6px 16px;
    border-bottom: 1px solid var(--separator);
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
</style>
