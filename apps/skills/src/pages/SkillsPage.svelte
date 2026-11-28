<!-- @page SkillsPage — the whole app: the skills, or the form one is added or changed in, with the question before one is removed; driven by one window's Skills state -->
<script lang="ts">
  import SkillForm from '../organisms/SkillForm.svelte'
  import SkillLibrary from '../organisms/SkillLibrary.svelte'
  import type { Skills } from '../lib/skills.svelte'

  interface Props {
    /** This window's state; the composition root builds it with the real `rpc` and `events`, a story with a fake. */
    skills: Skills
  }

  let { skills }: Props = $props()

  $effect(() => {
    void skills.load()
    return skills.listen()
  })
</script>

<main class="app" data-testid="skills-app">
  {#if !skills.loaded}
    <div class="waiting">
      {#if skills.failure}
        <ui-banner tone="error" data-testid="skills-failure">{skills.failure}</ui-banner>
      {:else}
        <ui-spinner size="large" label="Loading" data-testid="skills-loading"></ui-spinner>
      {/if}
    </div>
  {:else if skills.form}
    <SkillForm
      bind:form={skills.form}
      failure={skills.failure}
      savable={skills.savable}
      onsave={() => void skills.save()}
      oncancel={() => skills.close()}
      onremove={() => (skills.asking = true)}
    />
  {:else}
    <div class="rest">
      {#if skills.failure}
        <ui-banner tone="error" data-testid="skills-failure">{skills.failure}</ui-banner>
      {/if}
      <SkillLibrary skills={skills.skills} onedit={(skill) => skills.edit(skill)} />
    </div>
  {/if}
  {#if skills.asking && skills.form?.skill}
    <ui-alert
      open
      destructive
      action-label="Remove"
      heading="Remove {skills.form.skill.name}?"
      message="An app that asks for it is told there is no such skill. An agent given it before keeps its copy."
      data-testid="skills-confirm"
      onconfirm={() => void skills.remove()}
      oncancel={() => (skills.asking = false)}
    ></ui-alert>
  {/if}
</main>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg);
    color: var(--label);
    font: var(--type-body);
  }
  .rest {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .rest > :global(:last-child) {
    flex: 1;
    min-height: 0;
  }
  .waiting {
    flex: 1;
    display: grid;
    place-items: center;
    padding: 16px;
  }
</style>
