<!--
  @organism AppFrame — an installed app's window: the app's page runs in an
  `<iframe sandbox="allow-scripts allow-forms">` whose content the host serves at `/apps/<app>/static/` (with a policy that
  allows no network beyond what the app declared). The shell is the frame's host page: the app's calls reach its
  backend through `app.call`, its events, the shell's appearance and the window's display mode reach the app as
  messages in a dialect of MCP Apps, and a message is the frame's only when it comes from this iframe's own window
  (`shell/hostBridge.ts`). Atoms: ActivityIndicator, Button, Text.
  Test ids: `${appId}-opening` while the client mounts, `${appId}-failed` (+ `-retry`) when it cannot,
  `${appId}-frame` on the iframe itself and `${appId}-shield` on the cover while a press or a drag is going on.
-->
<script lang="ts">
  import { onDestroy, tick, untrack } from 'svelte'
  import { ActivityIndicator, Button, Text } from '@vos/ui/atoms'
  import { host } from '../lib/host.svelte'
  import { settings } from '../lib/settings.svelte'
  import { windowManager } from '../lib/windowManager.svelte'
  import { focusFrame } from '../shell/frameFocus'
  import { createHostBridge, isUpdateOf, routeToFrame, type HostBridge } from '../shell/hostBridge'
  import { contextChange, hostContextOf, type HostContext } from '../shell/hostContext'
  import { baseServices } from '../shell/hostServices'

  interface Props {
    appId: string
    /** The app's name, for the frame's accessible name. */
    name: string
    /** The shell's id for the window: the frame takes the keyboard when its window is the focused one. */
    windowId: string
    /** The browser features the app's frame asks for (`camera`, `clipboard-write`, ...): the iframe's `allow`. */
    permissions?: string[]
    /** How long the client has to mount before the window says it cannot (milliseconds). */
    patience?: number
    /** A stand-in document for the host's frame route (stories, where there is no host to serve it). */
    srcdoc?: string
  }

  let { appId, name, windowId, permissions = [], patience = 10_000, srcdoc }: Props = $props()

  let iframe: HTMLIFrameElement | undefined = $state()
  let phase = $state<'opening' | 'ready' | 'failed'>('opening')
  let failure = $state<string | null>(null)
  let attempt = $state(0)
  // a press anywhere in the shell, or a drag of something in it (a recent app dragged onto the stage: the browser takes
  // the pointer over then and ends the press with `pointercancel`): while it lasts the frame must not swallow the
  // pointer, so it is covered
  let pressing = $state(false)
  let dragging = $state(false)

  // The appearance the frame opens in. It is read once per attempt: the live appearance reaches the open frame as an
  // event, and reloading the app because the theme changed would lose what the person was doing.
  const src = $derived(
    `/apps/${appId}/static/?theme=${untrack(() => settings.appearance)}&attempt=${attempt}`
  )

  let bridge: HostBridge | undefined

  /** The window's display mode, from its layout: full screen is `fullscreen`, every other layout is `inline`. */
  const displayMode = (): 'inline' | 'fullscreen' =>
    windowManager.windows.find((w) => w.id === windowId)?.layout === 'fullscreen'
      ? 'fullscreen'
      : 'inline'
  const currentContext = (): HostContext =>
    hostContextOf({
      appearance: settings.appearance,
      displayMode: displayMode(),
      availableDisplayModes: ['inline', 'fullscreen']
    })
  let told: HostContext = untrack(currentContext)

  // A frame can navigate itself away (`location.href = ...`), which no policy stops, and then the iframe is not what the
  // shell loaded: its document was replaced. The host's bridge script says `hello` the moment it starts, before the client
  // does anything, so a document of the client says hello once per load. A second load of the same iframe that has not
  // been followed by a hello of its own is a page the frame went to: the window says the app left its page, with the way
  // back, rather than keep saying ready over a page the shell did not load. The hello was sent before any client code ran, so
  // how busy the client is cannot delay it; the short wait is only for the messages of an out-of-process frame, which may
  // arrive a moment after its load event.
  const HELLO_WAIT_MS = 1000
  let waiting: ReturnType<typeof setTimeout> | undefined
  function onframeload(event: Event): void {
    const element = event.currentTarget as HTMLIFrameElement
    // counted on the element itself: a frame is made again for every attempt, so every frame starts at 0
    const loads = Number(element.dataset.loads ?? 0) + 1
    element.dataset.loads = String(loads)
    if (loads < 2 || phase === 'failed') return
    clearTimeout(waiting)
    waiting = setTimeout(() => {
      if (phase === 'failed' || element !== iframe || Number(element.dataset.hellos ?? 0) >= loads)
        return
      failure = `${name} left its page, so the window cannot show it.`
      phase = 'failed'
    }, HELLO_WAIT_MS)
  }

  // One bridge per frame, made for this app and this iframe's window.
  $effect(() => {
    if (!iframe) return
    const frame = iframe
    const services = baseServices({
      context: currentContext,
      openLink: (url) => void window.open(url, '_blank', 'noopener,noreferrer'),
      setDisplayMode: (mode) => {
        windowManager.setLayout(windowId, mode === 'fullscreen' ? 'fullscreen' : 'floating')
        return displayMode()
      }
    })
    const made = createHostBridge({
      appId,
      frame: () => frame.contentWindow,
      post: (message) => frame.contentWindow?.postMessage(message, '*'),
      call: (method, params, options) => host.api.call(method, params, options),
      services,
      context: () => (told = currentContext()),
      // once the window says the app failed (it left its page, or never started) the frame is not listened to until Try again
      muted: () => phase === 'failed',
      onHello: () => {
        frame.dataset.hellos = String(Number(frame.dataset.hellos ?? 0) + 1)
      },
      onReady: () => {
        if (phase === 'failed') return
        phase = 'ready'
      },
      // the shell hears its own shortcut as it would have had the keys been in the shell's page
      onShortcut: (chord) => {
        window.dispatchEvent(
          new KeyboardEvent('keydown', { ...chord, bubbles: true, cancelable: true })
        )
      },
      onFailed: (message) => {
        failure = message
        phase = 'failed'
      }
    })
    bridge = made
    const listen = (e: MessageEvent): void => made.receive(e)
    window.addEventListener('message', listen)
    const stop = host.api.onNotification((method, params) => {
      routeToFrame(made, appId, method, params)
    })
    return () => {
      window.removeEventListener('message', listen)
      stop()
      made.dispose()
      bridge = undefined
    }
  })

  // The app was updated: the frame loads the new version's page, as Try again does (the old page's calls would meet a
  // backend that is not the one it was built for).
  $effect(() =>
    host.api.onNotification((method, params) => {
      if (isUpdateOf(appId, method, params)) attempt++
    })
  )

  // The host context, live: the shell's appearance and the window's display mode, as what changed.
  $effect(() => {
    const now = currentContext()
    if (phase !== 'ready') return
    const change = contextChange(told, now)
    told = now
    bridge?.contextChanged(change)
  })

  // The connection to the host dropped and came back (a restart, the network): a change made while it was away reached
  // nobody, so the app is told it can fetch again (`host:reconnected`, a `vos/event` that the app hears with `host.on`).
  let wasOffline = false
  $effect(() => {
    const connection = host.connection
    if (connection === 'offline') wasOffline = true
    else if (connection === 'online' && wasOffline) {
      wasOffline = false
      // the host forgot the frame's handlers on the bus with the old connection
      bridge?.rehandle()
      if (phase === 'ready') bridge?.event('host:reconnected', null)
    }
  })

  // A client that never mounts is a failure the person can retry.
  $effect(() => {
    void attempt
    phase = 'opening'
    failure = null
    const timer = setTimeout(() => {
      if (phase === 'opening') {
        failure = `${name} did not start.`
        phase = 'failed'
      }
    }, patience)
    return () => clearTimeout(timer)
  })

  // A click inside the frame never reaches the shell, so the window learns it was used from the page's focus: the
  // page loses it to the iframe.
  function onblur(): void {
    if (iframe && document.activeElement === iframe) windowManager.focus(windowId)
  }

  // A window opened from a launcher (the Dock) takes the keyboard. Focus that is already somewhere meaningful (a
  // field, a dialog, another window) stays put, so a window an agent opens never takes the keys from what the person
  // is typing.
  $effect(() => {
    if (phase !== 'ready' || windowManager.focusedId !== windowId) return
    void tick().then(() => {
      if (iframe) focusFrame(iframe)
    })
  })

  // Retry replaces the frame with a new one (the keyed block), and puts focus back on the button when it fails again.
  $effect(() => {
    if (phase !== 'failed' || attempt === 0) return
    void tick().then(() =>
      document.querySelector<HTMLElement>(`[data-testid="${appId}-failed-retry"]`)?.focus()
    )
  })

  // the frame is going away: the app may wrap up first (`ui/resource-teardown`); the window does not wait for long
  // Closing the window waits for that (a moment at most): once the frame is out of the page, nothing reaches it.
  $effect(() =>
    windowManager.beforeClose(windowId, () => bridge?.teardown('closed', 1000) ?? Promise.resolve())
  )
  onDestroy(() => {
    clearTimeout(waiting)
    bridge?.dispose()
  })
</script>

<svelte:window
  onpointerdowncapture={() => (pressing = true)}
  onpointerup={() => (pressing = false)}
  onpointercancel={() => (pressing = false)}
  ondragstartcapture={() => (dragging = true)}
  ondragend={() => (dragging = false)}
  ondrop={() => (dragging = false)}
  {onblur}
/>

<div class="frame">
  {#key attempt}
    <iframe
      bind:this={iframe}
      src={srcdoc === undefined ? src : undefined}
      {srcdoc}
      title={name}
      sandbox="allow-scripts allow-forms"
      allow={permissions.join('; ')}
      referrerpolicy="no-referrer"
      data-testid="{appId}-frame"
      onload={onframeload}
      class:hidden={phase !== 'ready'}
    ></iframe>
  {/key}
  {#if (pressing || dragging) && phase === 'ready'}
    <div class="shield" aria-hidden="true" data-testid="{appId}-shield"></div>
  {/if}
  {#if phase === 'failed'}
    <div class="state" role="alert" data-testid="{appId}-failed">
      <Text style="headline">Couldn’t open {name}</Text>
      {#if failure}<Text style="footnote" color="secondary">{failure}</Text>{/if}
      <Button testid="{appId}-failed-retry" size="small" capsule hit onclick={() => attempt++}
        >Try again</Button
      >
    </div>
  {:else if phase === 'opening'}
    <div class="state" data-testid="{appId}-opening">
      <ActivityIndicator size="large" label="Opening {name}" />
    </div>
  {/if}
</div>

<style>
  .frame {
    position: relative;
    width: 100%;
    height: 100%;
    min-height: 0;
  }
  iframe {
    display: block;
    width: 100%;
    height: 100%;
    border: 0;
    background: transparent;
    color-scheme: normal;
  }
  iframe.hidden {
    visibility: hidden;
  }
  .shield {
    position: absolute;
    inset: 0;
  }
  .state {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 24px;
    text-align: center;
  }
</style>
