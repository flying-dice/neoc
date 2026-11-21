<!--
  @organism NavigationStack — a push / pop stack of screens (UINavigationController): the top screen slides over
  the ones beneath, which stay mounted (so their scroll and state survive) but inert and hidden from assistive
  technology. The caller owns the list: it adds an item to push, marks one `leaving` to pop and removes it when
  `onleft` fires (after the slide, or at once with Reduce Motion). Focus moves to the new screen's title on push
  and back to where it was on pop. Test id: `testid` on the stack, `${testid}-screen-${id}` on each screen.
-->
<script lang="ts">
  import { onMount, tick, untrack, type Snippet } from 'svelte'

  interface Item {
    id: string
    /** Popped: it slides out, then `onleft(id)`. */
    leaving?: boolean
  }

  interface Props {
    testid: string
    items: readonly Item[]
    screen: Snippet<[string]>
    onleft?: (id: string) => void
  }

  let { testid, items, screen, onleft }: Props = $props()

  const SLIDE_MS = 350
  const reduced = (): boolean => matchMedia('(prefers-reduced-motion: reduce)').matches

  let root: HTMLElement | undefined = $state()
  // Screens present at mount do not animate in.
  let settled = $state(new Set<string>(untrack(() => items.map((i) => i.id))))
  let mounted = $state(false)
  const top = $derived([...items].reverse().find((i) => !i.leaving)?.id)

  /** The element that was focused before each push, to restore on pop. */
  const origins = new Map<string, Element | null>()
  const seen = new Set<string>(untrack(() => items.map((i) => i.id)))
  const timers = new Map<string, ReturnType<typeof setTimeout>>()

  onMount(() => {
    mounted = true
    return () => {
      for (const t of timers.values()) clearTimeout(t)
    }
  })

  $effect(() => {
    const ids = new Set(items.map((i) => i.id))
    // Pushed: remember the focus, then move it to the new screen's title.
    for (const item of items) {
      if (seen.has(item.id)) continue
      seen.add(item.id)
      origins.set(item.id, document.activeElement)
      if (mounted) {
        const id = item.id
        void tick().then(() => {
          const title = root?.querySelector<HTMLElement>(`[data-screen="${CSS.escape(id)}"] h1`)
          if (title) {
            title.tabIndex = -1
            title.focus({ preventScroll: true })
          }
        })
      }
    }
    // Popped: slide out, then hand it back; restore the focus the push took.
    for (const item of items) {
      if (!item.leaving || timers.has(item.id)) continue
      const id = item.id
      timers.set(
        id,
        setTimeout(
          () => {
            timers.delete(id)
            const origin = origins.get(id)
            origins.delete(id)
            seen.delete(id)
            onleft?.(id)
            if (origin instanceof HTMLElement && origin.isConnected) origin.focus({ preventScroll: true })
          },
          reduced() ? 0 : SLIDE_MS
        )
      )
    }
    for (const id of [...seen]) if (!ids.has(id) && !timers.has(id)) seen.delete(id)
  })

  // A screen that arrives after mount slides in once; mark it settled when the animation ends.
  function entered(id: string): void {
    settled = new Set(settled).add(id)
  }
</script>

<div class="ios-nav-stack" data-testid={testid} bind:this={root}>
  {#each items as item, i (item.id)}
    {@const isTop = item.id === top}
    <div
      class="screen"
      class:enter={mounted && !settled.has(item.id) && !item.leaving}
      class:leave={item.leaving}
      class:under={!isTop && !item.leaving}
      data-screen={item.id}
      data-testid="{testid}-screen-{item.id}"
      style:z-index={i + 1}
      inert={!isTop}
      aria-hidden={isTop ? undefined : true}
      onanimationend={() => entered(item.id)}
    >
      {@render screen(item.id)}
    </div>
  {/each}
</div>

<style>
  .ios-nav-stack {
    position: relative;
    width: 100%;
    height: 100%;
    min-height: 0;
    overflow: hidden;
    background: var(--bg-grouped-elevated);
  }
  .screen {
    position: absolute;
    inset: 0;
    background: var(--bg-grouped-elevated);
    transition:
      transform var(--duration) var(--ease-ios),
      opacity var(--duration) var(--ease-ios);
  }
  .screen.under {
    transform: translateX(-28%);
    opacity: 0.6;
    visibility: hidden;
    transition:
      transform var(--duration) var(--ease-ios),
      opacity var(--duration) var(--ease-ios),
      visibility 0s linear var(--duration);
  }
  .screen.enter {
    animation: push var(--duration) var(--ease-ios) both;
  }
  .screen.leave {
    animation: pop var(--duration) var(--ease-ios) both;
    pointer-events: none;
  }
  @keyframes push {
    from {
      transform: translateX(100%);
      box-shadow: -8px 0 24px rgb(0 0 0 / 0.12);
    }
  }
  @keyframes pop {
    to {
      transform: translateX(100%);
      box-shadow: -8px 0 24px rgb(0 0 0 / 0.12);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .screen,
    .screen.under {
      transition: none;
    }
    .screen.enter,
    .screen.leave {
      animation: none;
    }
  }
</style>
