<!--
  @organism ViewWindow — an installed app's window: opens a view instance on the host (`view.open`) when the
  shell window mounts, renders it with the managed renderer, and closes it (`view.close`) when the window goes.
  The app runs on the host; nothing of it runs here. Molecules: ActionToasts. Atoms: ActivityIndicator, Button,
  Text.
  Test ids: `${appId}-opening` while it opens, `${appId}-failed` (+ `-retry`) when it cannot, and the renderer's
  `${appId}-view` once it is open.
-->
<script lang="ts">
  import { onDestroy } from 'svelte'
  import { ActivityIndicator, Button, Text } from '@agentos/ui/atoms'
  import { ViewHost, type protocol, type ViewCore } from '../view'
  import ActionToasts, { type ActionToast } from '../molecules/ActionToasts.svelte'
  import { host } from '../lib/host.svelte'
  import { installedApp } from '../lib/apps.svelte'

  interface Props {
    appId: string
    /** The shell's id for the window (kept by the host so a reconnect maps back). */
    windowId: string
    kind: string
    params?: Record<string, unknown>
    onclosewindow: () => void
  }

  let { appId, windowId, kind, params, onclosewindow }: Props = $props()

  let box: HTMLElement | undefined = $state()
  let view = $state<ViewCore | null>(null)
  let failure = $state<string | null>(null)
  let toasts = $state<ActionToast[]>([])
  let attempt = $state(0)
  let toastId = 0
  let destroyed = false

  const name = $derived(installedApp(appId)?.name ?? appId)

  // Open once the box has a size (the host sizes the first render for it), and again on Retry.
  $effect(() => {
    void attempt
    if (!box || view) return
    const size = { w: Math.round(box.clientWidth), h: Math.round(box.clientHeight) }
    if (!size.w || !size.h) return
    failure = null
    void host
      .openView({
        app: appId,
        window: kind,
        windowId,
        size,
        ...(params ? { params: params as Record<string, protocol.Json> } : {})
      })
      .then(async (opened) => {
        if (destroyed) return void (await host.closeView(opened))
        view = opened
      })
      .catch((e: unknown) => {
        failure = e instanceof Error ? e.message : String(e)
      })
  })

  // Toasts with an action button belong to the view that raised them.
  $effect(() =>
    host.onToast((t) => {
      if (!view || t.viewId !== view.viewId) return
      const toast: ActionToast = { id: ++toastId, text: t.text, action: t.action }
      toasts.push(toast)
      setTimeout(() => dismiss(toast), t.duration ?? 6000)
    })
  )

  function dismiss(toast: ActionToast): void {
    const i = toasts.findIndex((t) => t.id === toast.id)
    if (i >= 0) toasts.splice(i, 1)
  }

  function act(toast: ActionToast): void {
    dismiss(toast)
    void view?.emit(`$toast/${toast.action.id}`, 'tap')
  }

  /** Only web links leave the shell, and never with the shell as opener. */
  function openUrl(url: string): void {
    if (/^https?:\/\//i.test(url)) window.open(url, '_blank', 'noopener,noreferrer')
  }

  onDestroy(() => {
    destroyed = true
    if (view) void host.closeView(view)
  })
</script>

<div class="frame" bind:this={box}>
  {#if view}
    <ViewHost {view} app={appId} onretry={() => host.retry()} onopenurl={openUrl} {onclosewindow} />
    <ActionToasts testid="{appId}-toast" {toasts} onaction={act} ondismiss={dismiss} />
  {:else if failure}
    <div class="state" role="alert" data-testid="{appId}-failed">
      <Text style="headline">Couldn’t open {name}</Text>
      <Text style="footnote" color="secondary">{failure}</Text>
      <Button testid="{appId}-failed-retry" size="small" capsule onclick={() => attempt++}
        >Try again</Button
      >
    </div>
  {:else}
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
  .state {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 24px;
    text-align: center;
  }
</style>
