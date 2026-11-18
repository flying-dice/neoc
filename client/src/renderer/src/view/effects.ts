/**
 * The DOM implementation of the imperative one-shots (`view.command`, client actions): focus and scroll a node
 * addressed by key path (found through its derived test id), copy to the clipboard. Opening URLs and closing the
 * window belong to the shell, which passes them in.
 */
import { testid } from './core/testid'
import type { ViewEffects } from './core/view'

export interface DomEffectsOptions {
  app: string
  /** The window's root element (found lazily: the effects are created before the view mounts). */
  root: () => HTMLElement | null | undefined
  openUrl?: (url: string) => void
  closeWindow?: () => void
  /** The browser's user-activation state (`navigator.userActivation`); a test passes its own. */
  userActivation?: () => { isActive: boolean } | undefined
}

export function createDomEffects(o: DomEffectsOptions): ViewEffects {
  const find = (path: string, rowKey?: string | number): HTMLElement | null => {
    const id = rowKey === undefined ? testid(o.app, path) : testid(o.app, `${path}/@row`, rowKey)
    return o.root()?.querySelector<HTMLElement>(`[data-testid="${CSS.escape(id)}"]`) ?? null
  }
  const reduced = (): boolean =>
    typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches
  return {
    focus: (path) => find(path)?.focus(),
    scrollTo: (path, rowKey) =>
      find(path, rowKey)?.scrollIntoView({
        block: 'nearest',
        behavior: reduced() ? 'auto' : 'smooth'
      }),
    copy: (text) => void navigator.clipboard?.writeText(text).catch(() => undefined),
    // A browser without the API (older Safari, Firefox) enforces the same rule inside `writeText` itself.
    userActive: () => (o.userActivation?.() ?? navigator.userActivation)?.isActive ?? true,
    openUrl: o.openUrl,
    closeWindow: o.closeWindow
  }
}
