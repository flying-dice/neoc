import { plainStore, type ViewStore } from './core/store'

/**
 * The reactive storage the core mutates: a deep `$state` proxy of the plain store. Every op, patch and draft the
 * core applies is then a fine-grained update: a `data.patch` for `state.display` repaints one text node and
 * nothing else. Pass it to the session: `new Session({ createStore: reactiveStore, ... })`.
 */
export function reactiveStore(): ViewStore {
  const store: ViewStore = $state(plainStore())
  return store
}
