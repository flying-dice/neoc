<!-- @page CopilotPage — the whole app: the copilots and their chats beside the open chat, a copilot's form, or the skill library; one of the two in a narrow window; or what to do when Docker cannot be reached; driven by one window's Copilots state -->
<script lang="ts">
  import { ChatThread } from '@vos/chat'
  import { EngineDown } from '@vos/docker'
  import CopilotForm from '../organisms/CopilotForm.svelte'
  import CopilotSidebar from '../organisms/CopilotSidebar.svelte'
  import SkillForm from '../organisms/SkillForm.svelte'
  import SkillLibrary from '../organisms/SkillLibrary.svelte'
  import type { Copilots } from '../lib/copilot.svelte'

  interface Props {
    /** This window's state; the composition root builds it with the real `rpc` and `events`, a story with a fake. */
    copilots: Copilots
    /** Open a web address the agent gave outside the app (the host's `open_link`). */
    onlink: (url: string) => void
  }

  let { copilots, onlink }: Props = $props()

  // in a window too narrow for both, the split view shows one column: the kit says when, the same for every app
  let narrow = $state(false)
  const open = $derived(copilots.open)
  const pane = $derived(copilots.pane)
  // what the sidebar marks: what the detail column shows
  const selected = $derived.by((): string | undefined => {
    if (pane === 'skills' || pane === 'skill') return 'skills'
    if (pane === 'copilot') return copilots.form?.copilot ? `settings:${copilots.form.copilot.id}` : 'add'
    if (open) return open.chat.id === '' ? `new:${open.chat.copilot_id}` : `chat:${open.chat.id}`
    return undefined
  })

  $effect(() => {
    void copilots.load()
    return copilots.listen()
  })
</script>

<main class="copilot" data-testid="copilot-app">
  {#if !copilots.loaded}
    <div class="nothing">
      {#if copilots.failure}
        <ui-banner tone="error" data-testid="copilot-failure">{copilots.failure}</ui-banner>
      {:else}
        <div class="waiting"><ui-spinner size="large" label="Loading" data-testid="copilot-loading"></ui-spinner></div>
      {/if}
    </div>
  {:else if copilots.engine && !copilots.engine.reachable}
    <EngineDown
      engine={copilots.engine}
      draft={copilots.hostDraft}
      ondraft={(address) => (copilots.hostDraft = address)}
      onconnect={() => void copilots.connect()}
      testid="copilot"
    />
  {:else}
    <!-- one column in a narrow window: what is open is in front of the list -->
    <ui-split-view column={open === null && pane === null ? 'sidebar' : 'detail'} oncollapsedchange={(e) => (narrow = e.detail.collapsed)}>
      <div slot="sidebar">
        <CopilotSidebar
          copilots={copilots.copilots}
          chats={copilots.chats}
          {selected}
          onstart={(copilot) => copilots.startChat(copilot)}
          onedit={(copilot) => copilots.editCopilot(copilot)}
          onopen={(chat) => void copilots.openChat(chat)}
          onlibrary={() => copilots.openLibrary()}
        />
      </div>
      <div>
        {#if pane === 'skill' && copilots.skillForm}
          <SkillForm
            bind:form={copilots.skillForm}
            failure={copilots.failure}
            savable={copilots.skillSavable}
            onsave={() => void copilots.saveSkill()}
            oncancel={() => copilots.closeSkill()}
            onremove={() => (copilots.asking = 'skill')}
          />
        {:else if pane === 'skills'}
          <SkillLibrary skills={copilots.skills} back={narrow} onedit={(skill) => copilots.editSkill(skill)} onback={() => copilots.back()} />
        {:else if pane === 'copilot' && copilots.form}
          <CopilotForm
            bind:form={copilots.form}
            copilot={copilots.editing}
            skills={copilots.skills}
            secrets={copilots.secrets}
            secretsProblem={copilots.secretsProblem}
            build={copilots.form.copilot ? copilots.buildOf(copilots.form.copilot.id) : null}
            failure={copilots.failure}
            savable={copilots.savable}
            onsave={() => void copilots.saveCopilot()}
            onbuild={() => void copilots.build()}
            oncancel={() => copilots.back()}
            onremove={() => (copilots.asking = 'copilot')}
          />
        {:else if open}
          <ChatThread
            conversation={open}
            partner={open.chat.copilot}
            draft={copilots.draft}
            failure={copilots.failure}
            back={narrow}
            ondraft={(text) => (copilots.draft = text)}
            onsend={() => void copilots.send()}
            onstop={() => void copilots.stop()}
            onanswer={(item, option) => void copilots.answer(item, option.option_id)}
            onremove={() => (copilots.asking = 'chat')}
            onback={() => copilots.back()}
            {onlink}
            testid="copilot"
          />
        {:else}
          <div class="nothing">
            {#if copilots.failure}
              <ui-banner tone="error" data-testid="copilot-failure">{copilots.failure}</ui-banner>
            {/if}
            <ui-empty-state
              icon="message"
              heading="No chat open"
              message={copilots.copilots.length === 0
                ? 'Add a copilot: a Dockerfile and the command of the agent it runs.'
                : 'Start a new chat with a copilot in the sidebar.'}
              data-testid="copilot-no-chat"
            >
              {#if copilots.copilots.length === 0}
                <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
                <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
                <ui-button slot="action" variant="filled" hit onclick={() => copilots.editCopilot(null)} data-testid="copilot-add-first">Add a copilot</ui-button>
              {/if}
            </ui-empty-state>
          </div>
        {/if}
      </div>
    </ui-split-view>
  {/if}
  {#if copilots.asking === 'chat' && open}
    <ui-alert
      open
      destructive
      action-label="Delete"
      heading="Delete this chat?"
      message="“{open.chat.title}”, everything in it, and its container and data are deleted."
      data-testid="copilot-confirm"
      onconfirm={() => void copilots.removeChat()}
      oncancel={() => (copilots.asking = null)}
    ></ui-alert>
  {:else if copilots.asking === 'copilot' && copilots.form?.copilot}
    <ui-alert
      open
      destructive
      action-label="Remove"
      heading="Remove {copilots.form.copilot.name}?"
      message="Its chats, their containers and volumes, and its images are removed with it."
      data-testid="copilot-confirm"
      onconfirm={() => void copilots.removeCopilot()}
      oncancel={() => (copilots.asking = null)}
    ></ui-alert>
  {:else if copilots.asking === 'skill' && copilots.skillForm?.skill}
    <ui-alert
      open
      destructive
      action-label="Remove"
      heading="Remove {copilots.skillForm.skill.name}?"
      message="It is taken out of the copilots that use it. Containers made before keep it."
      data-testid="copilot-confirm"
      onconfirm={() => void copilots.removeSkill()}
      oncancel={() => (copilots.asking = null)}
    ></ui-alert>
  {/if}
</main>

<style>
  .copilot {
    height: 100%;
    background: var(--bg);
    color: var(--label);
    font: var(--type-body);
  }
  .nothing {
    box-sizing: border-box;
    height: 100%;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding: 28px 12px 12px;
  }
  .waiting {
    height: 100%;
    display: grid;
    place-items: center;
  }
</style>
