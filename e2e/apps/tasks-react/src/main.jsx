import { createRoot } from 'react-dom/client'
import App from './App.jsx'
import { css } from './style.js'

/** The mount contract: the host calls this with the frame's root element and `{ rpc, host }`. */
export function mount(el, props) {
  const style = document.createElement('style')
  style.textContent = css
  document.head.append(style)
  const root = createRoot(el)
  root.render(<App {...props} />)
  return () => {
    root.unmount()
    style.remove()
  }
}
