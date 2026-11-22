import { mount } from 'svelte'
import App from './App.svelte'

// A normal Vite page: it starts itself, in the root element of `index.html`.
const root = document.getElementById('root')
if (root) mount(App, { target: root })
