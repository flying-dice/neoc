<!-- @organism SkillLibrary — the skills: each by its name and what it is for (or its files, when its SKILL.md does not say), a tap opens it to change, and the way to add one -->
<script lang="ts">
  import { filesOf } from '../lib/form'

  type Skill = VosApp.Skills.Skill

  interface Props {
    skills: readonly Skill[]
    /** Open the skill form: to change the skill given, or to add one (null). */
    onedit: (skill: Skill | null) => void
  }

  let { skills, onedit }: Props = $props()
</script>

<section class="library" aria-label="Skills" data-testid="skills-library">
  <header class="bar">
    <h1>Skills</h1>
  </header>
  <div class="fields">
    <ui-list variant="inset" label="Skills">
      <ui-section footer="A skill is a SKILL.md and the files beside it. The apps that run agents give their agents the skills chosen there.">
        {#each skills as skill (skill.id)}
          <ui-row heading={skill.name} subheading={skill.description || filesOf(skill)} tappable chevron onpress={() => onedit(skill)} data-testid="skills-skill-{skill.id}"></ui-row>
        {/each}
        <ui-row heading="Add a skill" action tappable onpress={() => onedit(null)} data-testid="skills-add"></ui-row>
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
