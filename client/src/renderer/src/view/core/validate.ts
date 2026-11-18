/**
 * Client-side validation of a text control (`TextField.validate {required, pattern, min, max, maxLength}`). It
 * only decides what the field shows (error text, `aria-invalid`); the host validates every write again, so this is
 * advice for the person typing, never a trust boundary. A bad pattern from the tree never throws.
 */
export interface ValidationRules {
  required?: boolean
  pattern?: string
  min?: number
  max?: number
  maxLength?: number
}

export function parseRules(value: unknown): ValidationRules {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return {}
  const v = value as Record<string, unknown>
  const num = (x: unknown): number | undefined =>
    typeof x === 'number' && Number.isFinite(x) ? x : undefined
  return {
    required: v.required === true,
    pattern: typeof v.pattern === 'string' ? v.pattern : undefined,
    min: num(v.min),
    max: num(v.max),
    maxLength: num(v.maxLength)
  }
}

/** The first problem with `text`, or null when it is fine. */
export function validateText(text: string, rules: ValidationRules, numeric = false): string | null {
  if (rules.required && text.trim() === '') return 'Required'
  if (text === '') return null
  if (rules.maxLength !== undefined && text.length > rules.maxLength)
    return `At most ${rules.maxLength} characters`
  if (rules.pattern) {
    try {
      if (!new RegExp(rules.pattern, 'u').test(text)) return 'Not in the expected format'
    } catch {
      /* an invalid pattern is the app's bug: do not block the person typing */
    }
  }
  if (numeric) {
    const n = Number(text)
    if (!Number.isFinite(n)) return 'Enter a number'
    if (rules.min !== undefined && n < rules.min) return `At least ${rules.min}`
    if (rules.max !== undefined && n > rules.max) return `At most ${rules.max}`
  }
  return null
}
