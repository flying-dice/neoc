<!--
  @organism ViewHost — one window's view instance on screen. It provides the view core to every adapter, renders the
  root node through the component registry and owns the window-level behaviour: dim and banner while offline
  (inputs still take drafts), toasts and live announcements, the inline error banner (the last good tree stays),
  window-scoped keyboard shortcuts, size class and the minute clock for `env.*`. A link the window asks to open
  (Markdown link, `openUrl`) never opens on its own: a confirmation shows the host and the full address first
  (focus starts on Cancel, and the address is in the dialog's description), and `onopenurl` runs only when the
  person says Open. While a confirmation shows, further links are ignored.
  Molecules: Node, Notices, ConnectionBanner, Alert. Atoms: Text.
  Test ids: `${app}-view`, `-tree`, `-view-error`, `-view-closed`, `-offline`, `-notice`, `-announcer`, and the link
  confirmation `${app}-link-confirm` (`-host`, `-url`, `-action-open`, `-action-cancel`).
-->
<script lang="ts">
  import { Text } from '@vos/ui/atoms'
  import { Alert } from '@vos/ui/molecules'
  import { untrack } from 'svelte'
  import { provideView } from '../context'
  import { describeLink, type ExternalLink } from '../core/link'
  import { pressesFocusedControl, watchInputModality } from '../core/modality'
  import type { ViewCore } from '../core/view'
  import { createDomEffects } from '../effects'
  import { defaultRegistry } from '../kit'
  import ConnectionBanner from '../molecules/ConnectionBanner.svelte'
  import Node from '../molecules/Node.svelte'
  import Notices from '../molecules/Notices.svelte'
  import type { ComponentRegistry } from '../registry'

  interface Props {
    view: ViewCore
    /** The app id: the prefix of every test id in this window. */
    app: string
    /** The window's name for a screen reader (the app's display name); the id when there is none. */
    label?: string
    registry?: ComponentRegistry
    /** Reconnect now (the offline banner's Retry). */
    onretry?: () => void
    /** Open a link the person confirmed (the shell decides how). */
    onopenurl?: (url: string) => void
    onclosewindow?: () => void
  }

  let {
    view,
    app,
    label,
    registry = defaultRegistry,
    onretry,
    onopenurl,
    onclosewindow
  }: Props = $props()

  let root: HTMLElement | undefined = $state()
  const store = $derived(view.store)
  const offline = $derived(store.link !== 'online')
  const queued = $derived(Object.keys(store.dirty).filter((k) => store.dirty[k]).length)

  // The context is static for the life of the window: the view, app and registry do not change under a mounted host.
  provideView({
    view: untrack(() => view),
    app: untrack(() => app),
    registry: untrack(() => registry),
    render: node,
    retry: untrack(() => onretry)
  })

  // Links wait for the person's say-so: the text of a link says nothing about where it goes. One confirmation
  // shows at a time and the destination it names never changes under the person's finger: a link asked for while
  // it is showing is dropped (an app that pushes links can neither swap the address before Open nor stack dialogs).
  let link = $state<ExternalLink | null>(null)
  let linkOpen = $state(false)
  function askToOpen(url: string): void {
    if (linkOpen) return
    const next = describeLink(url)
    if (!next) return // not a web or mail link: nothing to confirm, nothing to open
    link = next
    linkOpen = true
  }

  $effect(() => {
    view.setEffects(
      createDomEffects({ app, root: () => root, openUrl: askToOpen, closeWindow: onclosewindow })
    )
  })

  // env.sizeClass follows the container, so resizing a window never round-trips.
  $effect(() => {
    if (!root || typeof ResizeObserver === 'undefined') return undefined
    const ro = new ResizeObserver(([entry]) =>
      view.resize({
        w: Math.round(entry.contentRect.width),
        h: Math.round(entry.contentRect.height)
      })
    )
    ro.observe(root)
    return () => ro.disconnect()
  })

  // env.now ticks at minute resolution, so relative times update with zero messages.
  $effect(() => {
    const tick = (): void => void (store.env.now = Math.floor(Date.now() / 60_000) * 60_000)
    const id = setInterval(tick, 15_000)
    return () => clearInterval(id)
  })

  // env.scheme follows the document's theme (Settings > Appearance).
  $effect(() => {
    const read = (): void => {
      const explicit = document.documentElement.dataset.theme
      store.env.scheme =
        explicit === 'dark' || explicit === 'light'
          ? explicit
          : matchMedia('(prefers-color-scheme: dark)').matches
            ? 'dark'
            : 'light'
    }
    read()
    const mq = matchMedia('(prefers-color-scheme: dark)')
    mq.addEventListener('change', read)
    const mo = new MutationObserver(read)
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] })
    return () => {
      mq.removeEventListener('change', read)
      mo.disconnect()
    }
  })

  // How focus arrived (Tab or a pointer) decides what Enter does on a focused control; see core/modality.
  $effect(() => watchInputModality())

  const apple = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform)
  function onkeydown(e: KeyboardEvent): void {
    const t = e.target as HTMLElement | null
    const editable =
      !!t &&
      (t.isContentEditable ||
        t.tagName === 'INPUT' ||
        t.tagName === 'TEXTAREA' ||
        t.tagName === 'SELECT')
    // Enter or Space on a control the person walked to with the keyboard presses that control, as it always does;
    // a bare shortcut must not take it away. A control the person clicked or touched is not walked to (however the
    // browser styles its focus), so Enter after tapping still reaches a shortcut such as an app's "=".
    if (pressesFocusedControl(e, t)) return
    if (view.shortcuts.dispatch(e, { apple, editable })) e.preventDefault()
  }
</script>

{#snippet node(n: import('../protocol').Node, path: string, scope: import('../core/view').Scope)}
  <Node node={n} {path} {scope} />
{/snippet}

<!-- svelte-ignore a11y_no_noninteractive_element_interactions (window-scoped shortcuts listen on the window's root) -->
<section
  class="view"
  class:offline
  data-testid="{app}-view"
  aria-label={label ?? app}
  aria-busy={store.phase === 'opening'}
  tabindex="-1"
  data-window-focus
  bind:this={root}
  {onkeydown}
>
  {#if offline && store.phase !== 'closed'}
    <ConnectionBanner testid="{app}-offline" {queued} {onretry} />
  {/if}
  {#if store.error}
    <div class="error" role="alert" data-testid="{app}-view-error">
      <Text style="subheadline" color="destructive">{store.error}</Text>
    </div>
  {/if}
  {#if store.phase === 'closed'}
    <div class="closed" role="alert" data-testid="{app}-view-closed">
      <Text style="headline">This window is no longer available</Text>
      <Text style="subheadline" color="secondary">{store.closed ?? 'The app stopped.'}</Text>
    </div>
  {:else if store.tree}
    <div class="tree" data-testid="{app}-tree">
      <Node node={store.tree} path={store.tree.key} scope={{}} />
    </div>
  {/if}
  <Notices
    testid="{app}-notice"
    notices={store.notices}
    ondismiss={(id) => view.dismissNotice(id)}
    onaction={(id) => view.toastAction(id)}
    onundo={(id) => void view.undo(id)}
    onhold={(id) => view.holdNotice(id)}
    onrelease={(id) => view.releaseNotice(id)}
  />
  {#if link}
    {@const target = link}
    <Alert
      testid="{app}-link-confirm"
      bind:open={linkOpen}
      initialFocus="cancel"
      title={target.kind === 'web' ? 'Open this link?' : 'Write to this address?'}
      message={target.kind === 'web'
        ? 'It opens a page outside vos, in your browser.'
        : 'It opens your mail app.'}
      actions={[
        { label: 'Cancel', style: 'cancel' },
        { label: 'Open', preferred: true, onclick: () => onopenurl?.(target.url) }
      ]}
    >
      <span class="link-host" data-testid="{app}-link-confirm-host">{target.host}</span>
      <span class="link-url" data-testid="{app}-link-confirm-url">{target.url}</span>
    </Alert>
  {/if}
  <div class="sr-only" role="status" aria-live="polite" data-testid="{app}-announcer">
    {store.announcement}
  </div>
</section>

<style>
  .view {
    /* a focus target for the shell (a window that has just opened), not a control: it needs no ring */
    outline: none;
    position: relative;
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    min-height: 0;
    overflow: hidden;
    background: var(--bg-grouped-elevated);
  }
  .tree {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    transition: filter var(--duration) var(--ease-ios);
  }
  /* Offline: the window dims (it is stale, not gone). Typing still works; the banner says what is happening. */
  .offline .tree {
    filter: saturate(0.35) brightness(0.96);
  }
  .error {
    padding: 8px 16px;
    background: var(--bg-grouped-elevated-secondary, var(--bg-elevated));
    border-bottom: var(--hairline) solid var(--separator);
  }
  .closed {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
    padding: 24px;
    text-align: center;
  }
  .link-host {
    display: block;
    font: var(--type-headline);
    color: var(--label);
  }
  .link-url {
    display: block;
    margin-top: 4px;
    font: var(--type-caption1);
    font-family: var(--font-mono);
    color: var(--label-secondary);
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
</style>
