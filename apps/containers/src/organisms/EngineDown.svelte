<!-- @organism EngineDown — Docker cannot be reached: why, and the field that tries again or chooses another Docker -->
<script lang="ts">
  import type { Engine } from 'app:rpc'
  import ConnectionForm from '../molecules/ConnectionForm.svelte'

  interface Props {
    engine: Engine
    /** The address typed for another Docker. */
    draft: string
    ondraft: (address: string) => void
    onconnect: () => void
  }

  let { engine, draft, ondraft, onconnect }: Props = $props()
</script>

<section class="down" aria-label="Docker">
  <ui-empty-state icon="exclamationmark.triangle" heading="Docker Is Not Reachable" message={engine.problem} data-testid="containers-down"
  ></ui-empty-state>
  <div class="form">
    <ConnectionForm {draft} host={engine.host} {ondraft} {onconnect} />
    <p class="hint">
      Start Docker and try again, or give the address of a Docker elsewhere, like <code>tcp://nas.local:2375</code>.
    </p>
  </div>
</section>

<style>
  .down {
    box-sizing: border-box;
    height: 100%;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding: 28px 16px 16px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    overflow-y: auto;
  }
  .form {
    width: min(100%, 420px);
  }
  .hint {
    margin: 8px 0 0;
    font: var(--type-footnote);
    color: var(--label-secondary);
    text-align: center;
  }
  code {
    font-family: var(--font-mono);
  }
</style>
