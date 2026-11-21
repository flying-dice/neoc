import { mount as mountSvelte, unmount } from 'svelte'
import { createRpc, type MountProps } from '../generated/rpc'
import App from './App.svelte'

/** The mount contract: the host calls this with the frame's root element and `{ rpc, host }`. */
export function mount(el: HTMLElement, props: MountProps): () => void {
  const app = mountSvelte(App, { target: el, props: { rpc: createRpc(props.rpc), host: props.host } })
  return () => void unmount(app)
}
