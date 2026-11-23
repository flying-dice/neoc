<!-- @molecule A2uiSurface — a surface an agent drew (A2UI): its components as kit elements, its inputs kept in the surface's data, and a pressed button told to the agent -->
<script lang="ts">
  import {
    boundPath,
    absolute,
    childrenOf,
    mediaUrl,
    resolve,
    textOf,
    userAction,
    withValueAt,
    type Component,
    type Json,
    type Scope,
    type Surface,
    type UserAction
  } from '../lib/a2ui'

  interface Props {
    surface: Surface
    /** The agent is busy: nothing on the surface can be pressed. */
    disabled?: boolean
    onaction: (action: UserAction) => void
  }

  let { surface, disabled = false, onaction }: Props = $props()

  // the surface's data as the person has changed it: the agent's again whenever the agent sends the surface anew
  let edited = $state.raw<{ of: Surface; data: Json } | null>(null)
  const data = $derived(edited !== null && edited.of === surface ? edited.data : surface.data)
  // which tab of a Tabs, and whether a Modal is open, by component
  let shown = $state<Record<string, number | boolean>>({})

  // a component inside itself would never finish drawing
  const MAX_DEPTH = 24

  const testid = (id: string, scope: Scope): string => `agents-a2ui-${surface.id}-${id}${scope.replaceAll('/', '-')}`

  /** An input writes what the person entered to the path it is bound to; an unbound input has nowhere to write. */
  function write(property: Json | undefined, scope: Scope, value: Json): void {
    const path = boundPath(property)
    if (path !== undefined) edited = { of: surface, data: withValueAt(data, absolute(path, scope), value) }
  }

  function press(component: Component, scope: Scope): void {
    const action = userAction(surface, component, data, scope, new Date())
    if (action) onaction(action)
  }

  /** A heading written as Markdown (`## Title`, how A2UI 1.0 writes one) or by its variant (0.9). */
  function heading(component: Component, text: string): { level: number; text: string } {
    const marked = /^(#{1,5})\s+(.*)$/s.exec(text)
    if (marked) return { level: marked[1].length, text: marked[2] }
    const variant = /^h([1-5])$/.exec(typeof component.variant === 'string' ? component.variant : '')
    return { level: variant ? Number(variant[1]) : 0, text }
  }

  const FLEX: Record<string, string> = {
    start: 'flex-start',
    center: 'center',
    end: 'flex-end',
    stretch: 'stretch',
    spaceBetween: 'space-between',
    spaceAround: 'space-around',
    spaceEvenly: 'space-evenly'
  }
  const flex = (component: Component): string =>
    `justify-content:${FLEX[String(component.justify)] ?? 'flex-start'};align-items:${FLEX[String(component.align)] ?? 'stretch'}`
  const weight = (component: Component | undefined): string =>
    typeof component?.weight === 'number' && component.weight >= 0 ? `flex:${Math.min(component.weight, 1000)} 1 0` : ''
  const valueOf = (e: Event): string => (e.currentTarget as unknown as { value: string }).value
  const FIELD: Record<string, 'number' | 'password'> = { number: 'number', obscured: 'password' }
  const chosen = (component: Component, scope: Scope): string[] => {
    const value = resolve(component.value, data, scope)
    return Array.isArray(value) ? value.filter((v) => typeof v === 'string') : typeof value === 'string' ? [value] : []
  }
  const optionsOf = (component: Component, scope: Scope): { value: string; label: string }[] =>
    (Array.isArray(component.options) ? component.options : []).flatMap((option) =>
      typeof option === 'object' && option !== null && !Array.isArray(option) && typeof option.value === 'string'
        ? [{ value: option.value, label: textOf(option.label, data, scope) || option.value }]
        : []
    )
  const tabsOf = (component: Component, scope: Scope): { title: string; child: string }[] =>
    (Array.isArray(component.tabs) ? component.tabs : []).flatMap((tab) =>
      typeof tab === 'object' && tab !== null && !Array.isArray(tab) && typeof tab.child === 'string'
        ? [{ title: textOf(tab.title, data, scope), child: tab.child }]
        : []
    )
  const dateType = (component: Component): string =>
    component.enableDate === true && component.enableTime === true ? 'datetime-local' : component.enableTime === true ? 'time' : 'date'
</script>

{#snippet draw(id: string, scope: Scope, depth: number)}
  {@const component = surface.components.get(id)}
  {#if component && depth < MAX_DEPTH}
    {@const kind = component.component}
    {#if kind === 'Text'}
      {@const head = heading(component, textOf(component.text, data, scope))}
      {#if head.level > 0}
        <svelte:element this={`h${Math.min(head.level + 1, 6)}`} class="text h{head.level}">{head.text}</svelte:element>
      {:else}
        <p class="text" class:caption={component.variant === 'caption'}>{head.text}</p>
      {/if}
    {:else if kind === 'Row' || kind === 'Column' || kind === 'List'}
      {@const across = kind === 'Row' || (kind === 'List' && component.direction === 'horizontal')}
      <div class="box" class:across style={flex(component)}>
        {#each childrenOf(component.children, data, scope) as child, i (i)}
          <div class="cell" style={weight(surface.components.get(child.id))}>{@render draw(child.id, child.scope, depth + 1)}</div>
        {/each}
      </div>
    {:else if kind === 'Card'}
      <div class="card">{@render draw(String(component.child), scope, depth + 1)}</div>
    {:else if kind === 'Divider'}
      <hr class:upright={component.axis === 'vertical'} />
    {:else if kind === 'Image'}
      {@const url = mediaUrl(resolve(component.url, data, scope))}
      {#if url}
        <img
          class="image {typeof component.variant === 'string' ? component.variant : 'mediumFeature'}"
          src={url}
          alt={textOf(component.description, data, scope)}
          referrerpolicy="no-referrer"
          loading="lazy"
        />
      {/if}
    {:else if kind === 'Icon'}
      {@const name = textOf(component.name, data, scope)}
      <span class="icon" role="img" aria-label={name}>{name}</span>
    {:else if kind === 'Video'}
      {@const url = mediaUrl(resolve(component.url, data, scope))}
      {#if url}
        <!-- an agent's video comes without captions: there is no track to give -->
        <!-- svelte-ignore a11y_media_has_caption -->
        <video class="media" src={url} controls preload="none" data-testid={testid(id, scope)}></video>
      {/if}
    {:else if kind === 'AudioPlayer'}
      {@const url = mediaUrl(resolve(component.url, data, scope))}
      {#if url}
        <audio class="media" src={url} controls preload="none" aria-label={textOf(component.description, data, scope) || 'Audio'} data-testid={testid(id, scope)}
        ></audio>
      {/if}
    {:else if kind === 'Button'}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button
        variant={component.variant === 'primary' ? 'filled' : component.variant === 'borderless' ? 'plain' : 'tinted'}
        hit
        {disabled}
        onclick={() => press(component, scope)}
        data-testid={testid(id, scope)}
      >
        {@render draw(String(component.child), scope, depth + 1)}
      </ui-button>
    {:else if kind === 'TextField'}
      {@const label = textOf(component.label, data, scope)}
      <div class="field">
        <span class="caption" aria-hidden="true">{label}</span>
        <ui-text-field
          {label}
          type={FIELD[String(component.variant)] ?? 'text'}
          placeholder={textOf(component.placeholder, data, scope)}
          autocomplete="off"
          hit
          {disabled}
          value={textOf(component.value, data, scope)}
          oninput={(e: Event) => write(component.value, scope, component.variant === 'number' ? Number(valueOf(e)) : valueOf(e))}
          data-testid={testid(id, scope)}
        ></ui-text-field>
      </div>
    {:else if kind === 'DateTimeInput'}
      {@const label = textOf(component.label, data, scope) || 'Date'}
      <label class="field">
        <span class="caption">{label}</span>
        <input
          class="date"
          type={dateType(component)}
          {disabled}
          value={textOf(component.value, data, scope)}
          oninput={(e) => write(component.value, scope, e.currentTarget.value)}
          data-testid={testid(id, scope)}
        />
      </label>
    {:else if kind === 'CheckBox'}
      {@const label = textOf(component.label, data, scope)}
      <div class="check">
        <ui-toggle
          {label}
          hit
          {disabled}
          checked={resolve(component.value, data, scope) === true}
          onchange={(e: Event) => write(component.value, scope, (e.currentTarget as unknown as { checked: boolean }).checked)}
          data-testid={testid(id, scope)}
        ></ui-toggle>
        <span aria-hidden="true">{label}</span>
      </div>
    {:else if kind === 'ChoicePicker'}
      {@const options = optionsOf(component, scope)}
      {@const picked = chosen(component, scope)}
      {@const label = textOf(component.label, data, scope) || 'Choice'}
      <div class="field">
        {#if component.label !== undefined}<span class="caption">{label}</span>{/if}
        {#if component.variant === 'multipleSelection'}
          {#each options as option (option.value)}
            <div class="check">
              <ui-check
                label={option.label}
                hit
                {disabled}
                checked={picked.includes(option.value)}
                onchange={(e: Event) =>
                  write(
                    component.value,
                    scope,
                    (e.currentTarget as unknown as { checked: boolean }).checked
                      ? [...picked.filter((v) => v !== option.value), option.value]
                      : picked.filter((v) => v !== option.value)
                  )}
                data-testid="{testid(id, scope)}-{option.value}"
              ></ui-check>
              <span aria-hidden="true">{option.label}</span>
            </div>
          {/each}
        {:else}
          <ui-segmented
            {label}
            {options}
            hit
            {disabled}
            value={picked[0] ?? ''}
            onchange={(e: Event) => write(component.value, scope, [valueOf(e)])}
            data-testid={testid(id, scope)}
          ></ui-segmented>
        {/if}
      </div>
    {:else if kind === 'Slider'}
      {@const label = textOf(component.label, data, scope) || 'Value'}
      {@const value = Number(resolve(component.value, data, scope) ?? 0)}
      <label class="field">
        <span class="caption">{label}: {value}</span>
        <input
          type="range"
          min={typeof component.min === 'number' ? component.min : 0}
          max={typeof component.max === 'number' ? component.max : 100}
          {value}
          {disabled}
          oninput={(e) => write(component.value, scope, Number(e.currentTarget.value))}
          data-testid={testid(id, scope)}
        />
      </label>
    {:else if kind === 'Tabs'}
      {@const tabs = tabsOf(component, scope)}
      {@const at = Math.min(Number(shown[id] ?? 0), tabs.length - 1)}
      {#if tabs.length > 0}
        <div class="box">
          <ui-segmented
            label="Tabs"
            options={tabs.map((tab, i) => ({ value: String(i), label: tab.title || `Tab ${i + 1}` }))}
            hit
            value={String(at)}
            onchange={(e: Event) => (shown[id] = Number(valueOf(e)))}
            data-testid={testid(id, scope)}
          ></ui-segmented>
          {@render draw(tabs[at].child, scope, depth + 1)}
        </div>
      {/if}
    {:else if kind === 'Modal'}
      <!-- the trigger is the agent's own component, usually a button with no action: a press anywhere on it opens the content -->
      <div class="box">
        <div
          class="trigger"
          role="button"
          tabindex="0"
          aria-expanded={shown[id] === true}
          onclick={() => (shown[id] = shown[id] !== true)}
          onkeydown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') shown[id] = shown[id] !== true
          }}
          data-testid={testid(id, scope)}
        >
          {@render draw(String(component.trigger), scope, depth + 1)}
        </div>
        {#if shown[id] === true}
          <div class="card">{@render draw(String(component.content), scope, depth + 1)}</div>
        {/if}
      </div>
    {:else}
      <p class="text caption">This window cannot draw “{kind}”.</p>
    {/if}
  {/if}
{/snippet}

<div class="surface" data-testid="agents-surface-{surface.id}">
  {#if surface.components.has('root')}
    {@render draw('root', '', 0)}
  {:else}
    <ui-spinner label="The agent is drawing"></ui-spinner>
  {/if}
</div>

<style>
  .surface {
    align-self: stretch;
    padding: 12px;
    border-radius: var(--radius-lg);
    background: var(--bg-secondary);
    border: 1px solid var(--separator);
    overflow-wrap: anywhere;
  }
  .box {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }
  .box.across {
    flex-direction: row;
    flex-wrap: wrap;
  }
  .cell {
    min-width: 0;
  }
  .card {
    padding: 12px;
    border-radius: var(--radius-md, 10px);
    background: var(--bg);
    border: 1px solid var(--separator);
  }
  .text {
    margin: 0;
    white-space: pre-wrap;
  }
  .h1 {
    font: var(--type-title2, var(--type-headline));
  }
  .h2,
  .h3 {
    font: var(--type-headline);
  }
  .h4,
  .h5 {
    font: var(--type-subheadline, var(--type-body));
    font-weight: 600;
  }
  .caption {
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .date {
    align-self: flex-start;
    min-height: 36px;
    padding: 0 8px;
    border: 1px solid var(--separator);
    border-radius: var(--radius-md, 10px);
    background: var(--bg);
    color: var(--label);
    font: inherit;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .icon {
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  .image {
    display: block;
    max-width: 100%;
    max-height: 240px;
    border-radius: var(--radius-md, 10px);
    object-fit: cover;
  }
  .image.icon {
    width: 24px;
    height: 24px;
  }
  .image.avatar {
    width: 40px;
    height: 40px;
    border-radius: 50%;
  }
  .image.smallFeature {
    max-height: 96px;
  }
  .media {
    display: block;
    max-width: 100%;
  }
  .trigger {
    align-self: flex-start;
    cursor: pointer;
  }
  hr {
    align-self: stretch;
    margin: 0;
    border: 0;
    border-top: 1px solid var(--separator);
  }
  hr.upright {
    border-top: 0;
    border-inline-start: 1px solid var(--separator);
  }
</style>
