/**
 * The shared half of the text controls (TextField, SecureField, TextEditor): a text binding shown as a draft, with
 * write-back policy, composition handling, the caret kept where the person was typing when the host's merge is
 * adopted, and the "Saving…" status. Call it once from a component's script; it returns reactive getters and the
 * event handlers to put on the input element.
 *
 * The control is never server-controlled: it shows `binding.value` (the draft overlay first, then the server
 * value) and writes every edit into the draft; the core debounces the write, shields the field from remote
 * changes while it is dirty and adopts the merged value from the ack.
 */
import { tick } from 'svelte'
import type { ViewContext } from '../context'
import { TEXT_SYNC, type SyncPolicy } from '../core/bindings'
import { mapCaret } from '../core/textMerge'
import type { Scope } from '../core/view'
import type { Node } from '../protocol'

export interface TextControlOptions {
  ctx: ViewContext
  node: () => Node
  path: () => string
  scope: () => Scope
  /** The binding prop (default `text`). */
  prop?: string
  defaultSync?: SyncPolicy
  element: () => HTMLInputElement | HTMLTextAreaElement | undefined
  /** `Enter` commits and fires `submit` (single-line fields); a text editor keeps Enter for new lines. */
  submitOnEnter?: boolean
}

export function textControl(o: TextControlOptions) {
  const { view } = o.ctx
  const p = (name: string): unknown => view.evaluate(o.node().props?.[name], o.scope())
  const binding = $derived(view.bindingOf(o.node().props?.[o.prop ?? 'text'], o.scope()))
  const value = $derived.by(() => {
    const v = binding ? binding.value : p(o.prop ?? 'text')
    return v === null || v === undefined ? '' : String(v)
  })
  const policy = $derived(view.syncPolicy(p('sync'), o.defaultSync ?? TEXT_SYNC))

  // An ack replaced what was typed with the host's merged value: keep the caret where the person was typing.
  $effect(() => {
    return binding?.onAdopt(({ before, after }) => {
      const el = o.element()
      if (!el || document.activeElement !== el || typeof before !== 'string' || typeof after !== 'string') return
      const at = mapCaret(before, after, el.selectionStart ?? before.length)
      void tick().then(() => el.setSelectionRange(at, at))
    })
  })

  return {
    get binding() {
      return binding
    },
    get value() {
      return value
    },
    get readOnly() {
      return binding === null
    },
    get saving() {
      return binding?.dirty === true
    },
    get error() {
      return binding?.error
    },
    oninput(e: Event): void {
      binding?.set((e.currentTarget as HTMLInputElement | HTMLTextAreaElement).value, policy)
    },
    onfocus(): void {
      binding?.setFocused(true)
      view.fire(o.node(), o.path(), 'focusChange', o.scope(), { focused: true })
    },
    onblur(): void {
      binding?.setFocused(false)
      void binding?.commit()
      view.fire(o.node(), o.path(), 'focusChange', o.scope(), { focused: false })
    },
    async onkeydown(e: KeyboardEvent): Promise<void> {
      if (!o.submitOnEnter || e.key !== 'Enter' || e.isComposing) return
      await binding?.commit()
      view.fire(o.node(), o.path(), 'submit', o.scope(), { value })
    },
    oncompositionstart(): void {
      binding?.setComposing(true)
    },
    oncompositionend(): void {
      binding?.setComposing(false)
    }
  }
}
