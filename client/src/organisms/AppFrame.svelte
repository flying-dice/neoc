<!--
  @organism AppFrame — an installed app's window when the app has a client of its own: the client runs in an
  `<iframe sandbox="allow-scripts allow-forms">` whose document the host serves at `/frame/<app>` (with a policy that allows no
  network). The shell is the frame's only neighbour: the frame's `rpc` calls reach this app's backend through
  `app.call`, the backend's events and the shell's light or dark appearance come back as messages, and a message is
  the frame's only when it comes from this iframe's own window (`shell/frameBridge.ts`). Atoms: ActivityIndicator,
  Button, Text.
  Test ids: `${appId}-opening` while the client mounts, `${appId}-failed` (+ `-retry`) when it cannot,
  `${appId}-frame` on the iframe itself and `${appId}-shield` on the cover while a press or a drag is going on.
-->
<script lang="ts">
  import { onDestroy, tick, untrack } from 'svelte'
  import { ActivityIndicator, Button, Text } from '@vos/ui/atoms'
  import { host } from '../lib/host.svelte'
  import { settings } from '../lib/settings.svelte'
  import { windowManager } from '../lib/windowManager.svelte'
  import { createFrameBridge, eventFor, type FrameBridge } from '../shell/frameBridge'

  interface Props {
    appId: string
    /** The app's name, for the frame's accessible name. */
    name: string
    /** The shell's id for the window: the frame takes the keyboard when its window is the focused one. */
    windowId: string
    /** How long the client has to mount before the window says it cannot (milliseconds). */
    patience?: number
    /** A stand-in document for the host's frame route (stories, where there is no host to serve it). */
    srcdoc?: string
  }

  let { appId, name, windowId, patience = 10_000, srcdoc }: Props = $props()

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
    `/frame/${appId}?theme=${untrack(() => settings.appearance)}&attempt=${attempt}`
  )

  let bridge: FrameBridge | undefined

  // A frame can navigate itself away (`location.href = ...`), which no policy stops, and then the iframe is not what the
  // shell loaded: its document was replaced. The host's bridge script says `hello` the moment it starts, before the client
  // does anything, so a document of the client says hello once per load. A second load of the same iframe that has not
  // been preceded by a hello of its own is a page the frame went to: the window says the app left its page, with the way
  // back, rather than keep saying ready over a page the shell did not load. (No timer: a client that reloads itself and
  // then keeps the thread busy is still the client.)
  function onframeload(event: Event): void {
    const element = event.currentTarget as HTMLIFrameElement
    // counted on the element itself: a frame is made again for every attempt, so every frame starts at 0
    const loads = Number(element.dataset.loads ?? 0) + 1
    element.dataset.loads = String(loads)
    const hellos = Number(element.dataset.hellos ?? 0)
    if (loads < 2 || hellos >= loads || phase === 'failed') return
    failure = `${name} left its page, so the window cannot show it.`
    phase = 'failed'
  }

  // One bridge per frame, made for this app and this iframe's window.
  $effect(() => {
    if (!iframe) return
    const frame = iframe
    const made = createFrameBridge({
      appId,
      frame: () => frame.contentWindow,
      post: (message) => frame.contentWindow?.postMessage(message, '*'),
      call: (method, params) => host.api.call(method, params),
      onHello: () => {
        frame.dataset.hellos = String(Number(frame.dataset.hellos ?? 0) + 1)
      },
      onReady: () => {
        if (phase === 'failed') return
        phase = 'ready'
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
      const event = eventFor(appId, method, params)
      if (event) made.event(event.name, event.data)
    })
    return () => {
      window.removeEventListener('message', listen)
      stop()
      made.dispose()
      bridge = undefined
    }
  })

  // The shell's appearance, live.
  $effect(() => {
    const appearance = settings.appearance
    if (phase === 'ready') bridge?.event('host:theme', appearance)
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
      const active = document.activeElement
      if (active && active !== document.body && !active.closest('[data-launcher]')) return
      iframe?.contentWindow?.focus()
    })
  })

  // Retry replaces the frame with a new one (the keyed block), and puts focus back on the button when it fails again.
  $effect(() => {
    if (phase !== 'failed' || attempt === 0) return
    void tick().then(() =>
      document.querySelector<HTMLElement>(`[data-testid="${appId}-failed-retry"]`)?.focus()
    )
  })

  onDestroy(() => {
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
