import { mount as mountSvelte, unmount } from 'svelte'
import App from './App.svelte'

/** The mount contract: the generated entry binds `app:rpc` to the host, then calls this with the frame's root element. */
export function mount(el: HTMLElement): () => void {
  const app = mountSvelte(App, { target: el })
  return () => void unmount(app)
}
