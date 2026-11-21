<!-- @page TerminalPage — agent shell output (placeholder). -->
<script lang="ts">
  import { Text } from '@vos/ui/atoms'
  import { NavigationBar } from '@vos/ui/organisms'
  import { ScreenTemplate } from '@vos/ui/templates'

  const lines = [
    '$ agentos status',
    'research   running   62%   reading 5/14 papers',
    'coder      waiting         PR ready for review',
    'inbox      idle',
    'ops        running   20%   probing 3 services',
    '$ _'
  ]
</script>

<div class="terminal" data-theme="dark" data-testid="terminal">
  <ScreenTemplate background="plain">
    {#snippet navigationBar()}
      <NavigationBar testid="terminal-nav" title="Terminal" material />
    {/snippet}
    {#snippet content()}
      <pre class="output" data-testid="terminal-output">{#each lines as line, i (i)}<Text
            style="footnote"
            color="inherit"
            mono
            as="div">{line}</Text
          >{/each}</pre>
    {/snippet}
  </ScreenTemplate>
</div>

<style>
  .terminal {
    height: 100%;
  }
  .output {
    margin: 0;
    padding: 12px 18px;
    color: var(--ios-green);
    /* Wrap long lines: windows never scroll sideways (design language §8). */
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    -webkit-user-select: text;
    user-select: text;
  }
</style>
