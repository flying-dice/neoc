import { createRoot } from 'react-dom/client'
import { createRpc, type MountProps } from '../generated/rpc'
import App from './App'
import { css } from './style'

/** The mount contract: the host calls this with the frame's root element and `{ rpc, host }`. */
export function mount(el: HTMLElement, props: MountProps): () => void {
  const style = document.createElement('style')
  style.textContent = css
  document.head.append(style)
  const root = createRoot(el)
  root.render(<App rpc={createRpc(props.rpc)} host={props.host} />)
  return () => {
    root.unmount()
    style.remove()
  }
}
