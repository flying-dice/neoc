<!-- @molecule TypePicker — the first step of adding an agent: its type, Remote, Container or Program, each a row with its icon and what it is -->
<script lang="ts">
  import { KINDS, type Kind } from '../lib/kinds'

  interface Props {
    /** A type was chosen: the form for an agent of it opens. */
    onpick: (kind: Kind) => void
    oncancel: () => void
  }

  let { onpick, oncancel }: Props = $props()
</script>

<section class="types" aria-label="Choose the type of agent" data-testid="agents-types">
  <header class="bar">
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="plain" hit onclick={oncancel} data-testid="agents-types-cancel">Cancel</ui-button>
    <h1>New Agent</h1>
    <!-- the bar's title stays in its middle -->
    <span class="spacer" aria-hidden="true"></span>
  </header>
  <div class="list">
    <ui-list variant="inset" label="Types of agent">
      <ui-section header="How the chats reach it" footer="The type stays once the agent is added.">
        {#each KINDS as type (type.kind)}
          <ui-row
            heading={type.label}
            icon={type.icon}
            icon-color={type.tint}
            subheading={type.explanation}
            tappable
            chevron
            label="A {type.label.toLowerCase()} agent"
            onpress={() => onpick(type.kind)}
            data-testid="agents-type-{type.kind}"
          ></ui-row>
        {/each}
      </ui-section>
    </ui-list>
  </div>
</section>

<style>
  .types {
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
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 16px 12px;
  }
</style>
