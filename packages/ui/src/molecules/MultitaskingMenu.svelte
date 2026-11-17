<!--
  @molecule MultitaskingMenu — the iPadOS "···" popover: a row of layout
  choices (Full Screen, Split View, Slide Over, Window) plus window actions.
  Atoms: Material, Icon.
  Test ids: `testid` on the menu, `${testid}-<layout>` on each layout
  (fullscreen, split-left, split-right, slideover, floating), `${testid}-minimize`,
  `${testid}-close`.
-->
<script lang="ts">
  import Material from '../atoms/Material.svelte'
  import Icon from '../atoms/Icon.svelte'
  import type { IconName } from '../icons'
  import type { WindowLayout } from '../types'

  interface Props {
    testid: string
    layout: WindowLayout
    title?: string
    onlayout?: (layout: WindowLayout) => void
    onminimize?: () => void
    onclose?: () => void
  }

  let { testid, layout, title, onlayout, onminimize, onclose }: Props = $props()

  const options: { value: WindowLayout; label: string; icon: IconName }[] = [
    { value: 'fullscreen', label: 'Full Screen', icon: 'layout.fullscreen' },
    { value: 'split-left', label: 'Split View Left', icon: 'layout.split.left' },
    { value: 'split-right', label: 'Split View Right', icon: 'layout.split.right' },
    { value: 'slideover', label: 'Slide Over', icon: 'layout.slideover' },
    { value: 'floating', label: 'Window', icon: 'layout.window' }
  ]
</script>

<div class="mt-menu" role="menu" aria-label="{title ?? 'Window'} multitasking" data-testid={testid}>
  <Material thickness="thick" radius="14px">
    <div class="layouts" role="group" aria-label="Layout">
      {#each options as o (o.value)}
        <button
          type="button"
          role="menuitemradio"
          data-testid="{testid}-{o.value}"
          aria-checked={layout === o.value}
          aria-label={o.label}
          title={o.label}
          class="layout"
          class:current={layout === o.value}
          onclick={() => onlayout?.(o.value)}
        >
          <Icon name={o.icon} size={26} weight={1.8} />
        </button>
      {/each}
    </div>
    <div class="actions">
      <button type="button" role="menuitem" class="action" data-testid="{testid}-minimize" onclick={onminimize}>
        <span>Minimize</span><Icon name="minus" size={17} weight={2.2} />
      </button>
      <button type="button" role="menuitem" class="action destructive" data-testid="{testid}-close" onclick={onclose}>
        <span>Close</span><Icon name="xmark" size={15} weight={2.4} />
      </button>
    </div>
  </Material>
</div>

<style>
  .mt-menu {
    width: 250px;
    border-radius: 14px;
    box-shadow:
      0 0 0 0.5px rgb(0 0 0 / 0.1),
      0 14px 40px rgb(0 0 0 / 0.22);
    color: var(--label);
  }
  .layouts {
    display: flex;
    justify-content: space-between;
    padding: 8px;
  }
  .layout {
    appearance: none;
    border: 0;
    background: transparent;
    width: 44px;
    height: 40px;
    display: grid;
    place-items: center;
    border-radius: 9px;
    color: var(--label-secondary);
    cursor: pointer;
  }
  .layout:hover {
    background: var(--fill-tertiary);
    color: var(--label);
  }
  .layout.current {
    color: var(--tint);
  }
  .actions {
    box-shadow: 0 calc(var(--hairline) * -1) 0 var(--separator);
  }
  .action {
    appearance: none;
    border: 0;
    background: transparent;
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 44px;
    padding: 0 16px;
    font: var(--type-body);
    letter-spacing: var(--tracking-body);
    color: var(--label);
    cursor: pointer;
  }
  .action + .action {
    box-shadow: 0 calc(var(--hairline) * -1) 0 var(--separator);
  }
  .action:hover {
    background: var(--fill-quaternary);
  }
  .destructive {
    color: var(--ios-red);
  }
</style>
