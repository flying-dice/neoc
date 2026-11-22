import { createRoot } from 'react-dom/client'
import App from './App'
import './style.css'

// A normal Vite page: it starts itself, in the root element of `index.html`.
const root = document.getElementById('root')
if (root) createRoot(root).render(<App />)
