<!--
  @organism SessionBar — one slim row of session controls above the composer:
  mode, every ACP config option the agent exposes (model, effort, …), slash
  commands, and a context-usage ring. Only what the agent supports is shown.
  Atoms: UsageRing. Molecules: SessionPill, SessionToggle.
  Test ids: `agents-mode`, `agents-config-<optionId>`, `agents-commands`, `agents-usage`.
-->
<script lang="ts">
  import type { IconName } from '@agentos/ui'
  import type { ConfigValue, SessionConfigOption, SessionModeState } from '@agentos/sdk/host/client'
  import UsageRing from '../atoms/UsageRing.svelte'
  import SessionPill, { type PillOption } from '../molecules/SessionPill.svelte'
  import SessionToggle from '../molecules/SessionToggle.svelte'
  import type { Conversation, SlashCommand } from '../lib/agents.svelte'

  interface Props {
    modes: SessionModeState | null
    configOptions: SessionConfigOption[]
    commands: SlashCommand[]
    usage: Conversation['usage'] | null
    disabled?: boolean
    onmode: (modeId: string) => void
    onconfig: (configId: string, value: ConfigValue) => void
    oncommand: (command: SlashCommand) => void
  }

  let {
    modes,
    configOptions,
    commands,
    usage,
    disabled = false,
    onmode,
    onconfig,
    oncommand
  }: Props = $props()

  const ICONS: Record<string, IconName> = {
    mode: 'switch.2',
    model: 'cpu',
    model_config: 'gear',
    thought_level: 'sparkles'
  }

  /** Modes are often also exposed as a config option; show them once. */
  const modeIsConfig = $derived(configOptions.some((o) => o.category === 'mode'))
  const modeOptions = $derived<PillOption[]>(
    modes?.availableModes.map((m) => ({ value: m.id, name: m.name, description: m.description })) ??
      []
  )

  function selectOptions(option: Extract<SessionConfigOption, { type: 'select' }>): PillOption[] {
    return option.options.flatMap((o) =>
      'group' in o
        ? o.options.map((x) => ({
            value: x.value,
            name: x.name,
            description: x.description,
            group: o.name
          }))
        : [{ value: o.value, name: o.name, description: o.description }]
    )
  }

  const visible = $derived(!!modes || configOptions.length > 0 || commands.length > 0 || !!usage)
</script>

{#if visible}
  <div class="session-bar">
    <div class="controls">
      {#if modes && !modeIsConfig && modeOptions.length > 1}
        <SessionPill
          testid="agents-mode"
          label="Mode"
          icon="switch.2"
          value={modes.currentModeId}
          options={modeOptions}
          {disabled}
          onchange={onmode}
        />
      {/if}
      {#each configOptions as option (option.id)}
        {#if option.type === 'select'}
          <SessionPill
            testid="agents-config-{option.id}"
            label={option.name}
            icon={ICONS[option.category ?? ''] ?? 'gear'}
            value={option.currentValue}
            options={selectOptions(option)}
            {disabled}
            onchange={(v) => onconfig(option.id, v)}
          />
        {:else if option.type === 'boolean'}
          <SessionToggle
            testid="agents-config-{option.id}"
            label={option.name}
            checked={option.currentValue}
            {disabled}
            onchange={(v) => onconfig(option.id, v)}
          />
        {/if}
      {/each}
      {#if commands.length}
        <SessionPill
          testid="agents-commands"
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
      <span data-testid="agents-usage"
        ><UsageRing used={usage.used} size={usage.size} cost={usage.cost} /></span
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
  }
  .controls {
    display: flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
    overflow-x: auto;
    scrollbar-width: none;
  }
</style>
