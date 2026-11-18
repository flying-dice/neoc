/**
 * The starter kit: the adapters registered by default. Wave A3 ships the set needed to demonstrate the renderer
 * (layout, text, button, text field, toggle, list with rows, screen); wave B1 registers the rest of tier 1 here
 * with one `register` call each. Every adapter wraps an `@agentos/ui` component and passes the derived test id.
 */
import Button from './molecules/Button.svelte'
import ForEach from './molecules/ForEach.svelte'
import Group from './molecules/Group.svelte'
import Show from './molecules/Show.svelte'
import Stack from './molecules/Stack.svelte'
import Text from './molecules/Text.svelte'
import TextField from './molecules/TextField.svelte'
import Toggle from './molecules/Toggle.svelte'
import Row from './organisms/Row.svelte'
import Screen from './pages/Screen.svelte'
import { ComponentRegistry, createRegistry } from './registry'
import List from './templates/List.svelte'
import { registerControls } from './kit/controls'

/** Register the starter set on a registry (so a story or test can start from it and add its own). */
export function registerStarterKit(registry: ComponentRegistry): ComponentRegistry {
  registry
    .register('Stack', { component: Stack, since: '1.0' })
    .register('Group', { component: Group, since: '1.0' })
    .register('Show', { component: Show, since: '1.0' })
    .register('Text', { component: Text, since: '1.0' })
    .register('Button', { component: Button, since: '1.0' })
    .register('TextField', { component: TextField, since: '1.0', bindings: ['text'] })
    .register('Toggle', { component: Toggle, since: '1.0', bindings: ['on'] })
    .register('List', { component: List, since: '1.0', bindings: ['selection'] })
    .register('ForEach', { component: ForEach, since: '1.0' })
    .register('Row', { component: Row, since: '1.0' })
    .register('Screen', { component: Screen, since: '1.0' })
  return registerControls(registry)
}

/** The registry a window uses unless it is given another. */
export const defaultRegistry: ComponentRegistry = registerStarterKit(createRegistry())
