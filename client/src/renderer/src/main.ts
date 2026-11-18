import { mount } from 'svelte'
import '@agentos/ui/styles.css'
import './app.css'
import App from './App.svelte'
// Start routing main → app events before any app window mounts.
import './lib/appRuntime'

const app = mount(App, {
  target: document.getElementById('app')!
})

export default app
