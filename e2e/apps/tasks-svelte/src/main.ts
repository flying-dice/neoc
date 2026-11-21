import { mount as mountSvelte, unmount } from 'svelte'
import App from './App.svelte'
import type { Host, Rpc } from './vos'

/** The mount contract: the host calls this with the frame's root element and `{ rpc, host }`. */
export function mount(el: HTMLElement, props: { rpc: Rpc; host: Host }): () => void {
  const app = mountSvelte(App, { target: el, props })
  return () => void unmount(app)
}
