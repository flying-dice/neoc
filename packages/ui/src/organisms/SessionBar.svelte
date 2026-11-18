<!--
  @organism SessionBar — one slim row of session controls above the composer: mode, every setting the agent
  exposes (model, effort, ...), slash commands, and a context-usage ring. Only what the agent supports is shown;
  the row wraps in a narrow window instead of scrolling sideways.
  Atoms: UsageRing. Molecules: SessionPill, SessionToggle.
  Test ids: `${testid}-mode`, `${testid}-config-<id>`, `${testid}-commands`, `${testid}-usage`.
-->
<script lang="ts">
  import type { IconName } from '../icons'
  import SessionPill, { type PillOption } from '../molecules/SessionPill.svelte'
  import SessionToggle from '../molecules/SessionToggle.svelte'
  import UsageRing from '../atoms/UsageRing.svelte'
  import type { ChatCommand, ChatMode, ChatSetting, ChatUsage } from '../types'

  interface Props {
    testid: string
    modes: ChatMode | null
    settings: ChatSetting[]
    commands: ChatCommand[]
    usage: ChatUsage | null
    disabled?: boolean
    onmode: (modeId: string) => void
    onsetting: (id: string, value: string | boolean) => void
    oncommand: (command: ChatCommand) => void
  }

  let {
    testid,
    modes,
    settings,
    commands,
    usage,
    disabled = false,
    onmode,
    onsetting,
    oncommand
  }: Props = $props()

  const ICONS: Record<string, IconName> = {
    mode: 'switch.2',
    model: 'cpu',
    model_config: 'gear',
    thought_level: 'sparkles'
  }

  /** Modes are often also exposed as a setting; show them once. */
  const modeIsSetting = $derived(settings.some((s) => s.category === 'mode'))
  const modeOptions = $derived<PillOption[]>(
    modes?.available.map((m) => ({ value: m.id, name: m.name, description: m.description })) ?? []
  )
  const visible = $derived(!!modes || settings.length > 0 || commands.length > 0 || !!usage)
</script>

{#if visible}
  <div class="session-bar" role="group" aria-label="Session settings" data-testid={testid}>
    <div class="controls">
      {#if modes && !modeIsSetting && modeOptions.length > 1}
        <SessionPill
          testid="{testid}-mode"
          label="Mode"
          icon="switch.2"
          value={modes.current}
          options={modeOptions}
          {disabled}
          onchange={onmode}
        />
      {/if}
      {#each settings as setting (setting.id)}
        {#if setting.kind === 'select'}
          <SessionPill
            testid="{testid}-config-{setting.id}"
            label={setting.name}
            icon={ICONS[setting.category ?? ''] ?? 'gear'}
            value={setting.current}
            options={setting.options}
            {disabled}
            onchange={(v) => onsetting(setting.id, v)}
          />
        {:else}
          <SessionToggle
            testid="{testid}-config-{setting.id}"
            label={setting.name}
            checked={setting.current}
            {disabled}
            onchange={(v) => onsetting(setting.id, v)}
          />
        {/if}
      {/each}
      {#if commands.length}
        <SessionPill
          testid="{testid}-commands"
          label="Commands"
          value=""
          placeholder="/"
          options={commands.map((c) => ({
            value: c.name,
            name: `/${c.name}`,
            description: c.description
          }))}
          {disabled}
          onchange={(name) => {
            const command = commands.find((c) => c.name === name)
            if (command) oncommand(command)
          }}
        />
      {/if}
    </div>
    {#if usage}
      <span class="usage" data-testid="{testid}-usage"
        ><UsageRing used={usage.used} size={usage.size} cost={usage.cost ?? undefined} /></span
      >
    {/if}
  </div>
{/if}

<style>
  .session-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 4px 10px 0;
    min-width: 0;
  }
  .controls {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 2px;
    min-width: 0;
  }
  .usage {
    flex: none;
  }
</style>
