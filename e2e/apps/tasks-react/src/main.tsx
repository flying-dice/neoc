import { createRoot } from 'react-dom/client'
import App from './App'
import { css } from './style'

/** The mount contract: the generated entry binds `app:rpc` to the host, then calls this with the frame's root element. */
export function mount(el: HTMLElement): () => void {
  // the client's styles are part of its one script: added to the frame's document when the app mounts
  const style = document.createElement('style')
  style.textContent = css
  document.head.append(style)
  const root = createRoot(el)
  root.render(<App />)
  return () => {
    root.unmount()
    style.remove()
  }
}
