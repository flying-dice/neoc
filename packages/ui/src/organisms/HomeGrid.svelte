<!--
  @organism HomeGrid — the iPadOS Home Screen's grid of apps: each an icon over its name, in the order given, laid out
  in as many columns as the width allows (four on a phone, up to eight) and scrolling down inside itself when there
  are more rows than fit. It takes the size of the box it is put in.
  Molecules: AppIcon.
  Test ids: `testid` on the list, `${testid}-${app.id}` on each icon, `${testid}-${app.id}-name` on the name under it.

  Keyboard: the grid is one stop in the tab order (roving focus: the icon last focused, the first until then). The
  arrow keys move across it as across a grid of icons: Left and Right in reading order, Up and Down within the column;
  Home and End go to the ends of the row, with Ctrl or Cmd to the first and last app. Enter or Space opens.
  A focused icon is scrolled into view.

  Names: the button is named by the app (`aria-label`); the visible name under it is for the eye (`aria-hidden`), white
  on a dark capsule so it holds 4.5:1 on any wallpaper, in either theme.

  `onmenu` gives every icon a context menu (see AppIcon): it is called with the app's id and the icon's button.
-->
<script lang="ts">
  import AppIcon from '../molecules/AppIcon.svelte'
  import { gridIndex } from '../keys'
  import type { DockApp } from '../types'

  interface Props {
    testid: string
    apps: DockApp[]
    /** What the list is, for assistive technology. */
    label?: string
    onselect?: (id: string) => void
    /** Ask for an icon's context menu. */
    onmenu?: (id: string, anchor: HTMLElement) => void
  }

  let { testid, apps, label = 'Apps', onselect, onmenu }: Props = $props()

  let list: HTMLElement | undefined = $state()
  /** The id of the app whose icon holds the tab stop (an app that went away gives it to the first). */
  let stop = $state<string | null>(null)
  const tabStop = $derived(apps.some((a) => a.id === stop) ? stop : (apps[0]?.id ?? null))

  const buttons = (): HTMLElement[] => [...(list?.querySelectorAll<HTMLElement>('button') ?? [])]

  /** How many icons the first row holds, as it is laid out now. */
  function columns(all: HTMLElement[]): number {
    const top = all[0]?.getBoundingClientRect().top ?? 0
    const firstBelow = all.findIndex((b) => b.getBoundingClientRect().top > top + 1)
    return firstBelow < 0 ? all.length : firstBelow
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.altKey || e.shiftKey) return
    const all = buttons()
    const at = all.indexOf(document.activeElement as HTMLElement)
    if (at < 0) return
    const next = gridIndex(all.length, columns(all), at, e.key, e.ctrlKey || e.metaKey)
    if (next === null || ((e.ctrlKey || e.metaKey) && e.key !== 'Home' && e.key !== 'End')) return
    e.preventDefault()
    all[next]?.focus()
    all[next]?.scrollIntoView({ block: 'nearest' })
  }

  function onfocusin(e: FocusEvent): void {
    const id = (e.target as HTMLElement).closest<HTMLElement>('[data-app]')?.dataset.app
    if (id !== undefined) stop = id
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions (the arrow keys move between the icons, which are the interactive parts) -->
<ul class="ios-home-grid" aria-label={label} data-testid={testid} bind:this={list} {onkeydown} {onfocusin}>
  {#each apps as app (app.id)}
    <li class="cell" data-app={app.id}>
      <AppIcon
        testid="{testid}-{app.id}"
        name={app.name}
        icon={app.icon}
        background={app.background}
        size={60}
        tabindex={tabStop === app.id ? 0 : -1}
        onclick={() => onselect?.(app.id)}
        onmenu={onmenu ? (anchor) => onmenu(app.id, anchor) : undefined}
      />
      <span class="name" aria-hidden="true" data-testid="{testid}-{app.id}-name">{app.name}</span>
    </li>
  {/each}
</ul>

<style>
  .ios-home-grid {
    list-style: none;
    margin: 0 auto;
    /* room for an icon's focus ring at every edge of the scrolling box */
    padding: 8px;
    box-sizing: border-box;
    width: 100%;
    max-width: 736px; /* eight columns at most */
    max-height: 100%;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(72px, 1fr));
    gap: 22px 12px;
    align-content: start;
    overflow-y: auto;
    overscroll-behavior: contain;
    scrollbar-width: none;
  }
  .cell {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .name {
    max-width: 100%;
    padding: 1px 6px;
    border-radius: var(--radius-pill);
    /* white on this holds 4.5:1 over the lightest part of any wallpaper (5.4:1 on #12c2e9) */
    background: rgb(0 0 0 / 0.42);
    color: #fff;
    font: var(--type-caption1);
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    pointer-events: none;
  }
  /* Reduce Transparency: a solid plate */
  :global(.reduce-transparency) .name {
    background: #2c2c2e;
  }
</style>
