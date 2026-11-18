import { z } from 'zod'
import { defineServerApp } from '@agentos/sdk/app/server'
import { evaluate, formatDisplay } from '../shared/evaluate'

interface HistoryEntry {
  expression: string
  result: number
}

/**
 * Calculator server module. Registers tools agents can use, and handlers its
 * own window calls. Keeps a short calculation history in app storage.
 */
export default defineServerApp((ctx) => {
  const history = (): HistoryEntry[] => ctx.storage.get<HistoryEntry[]>('history') ?? []
  const record = (entry: HistoryEntry): void =>
    ctx.storage.set('history', [entry, ...history()].slice(0, 50))

  // ── Renderer → main ───────────────────────────────────────────────────
  ctx.handle('evaluate', (payload) => {
    const expression = z.string().parse(payload)
    const result = evaluate(expression)
    record({ expression, result })
    return result
  })
  ctx.handle('history', () => history())

  // ── Tools (agents via MCP, and any app client) ──────────────────────
  ctx.registerTool({
    name: 'calculate',
    title: 'Calculate',
    description:
      'Evaluate an arithmetic expression exactly. Supports + - * / % ^, parentheses, unary minus and decimals. Use this instead of doing arithmetic in your head.',
    input: { expression: z.string().describe('Arithmetic expression, e.g. "(12.5 + 7) * 3"') },
    readOnly: true,
    run: ({ expression }) => {
      const result = evaluate(expression)
      record({ expression, result })
      return { expression, result, display: formatDisplay(result) }
    }
  })

  ctx.registerTool({
    name: 'show_on_calculator',
    title: 'Show on Calculator',
    description:
      "Open the Calculator window and show a value or expression result on its display, so the user can see it and keep calculating.",
    input: {
      expression: z.string().describe('Expression or number to evaluate and display')
    },
    run: ({ expression }) => {
      const result = evaluate(expression)
      record({ expression, result })
      ctx.openWindow()
      ctx.emit('display', { expression, result })
      return { shown: formatDisplay(result) }
    }
  })

  ctx.registerTool({
    name: 'history',
    title: 'Calculation History',
    description: 'List recent calculations made in the Calculator (newest first).',
    input: { limit: z.number().int().min(1).max(50).default(10) },
    readOnly: true,
    run: ({ limit }) => ({ entries: history().slice(0, limit) })
  })
})
