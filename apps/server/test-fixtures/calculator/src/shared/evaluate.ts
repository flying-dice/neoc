/**
 * Safe arithmetic evaluator (no `eval`): + − × ÷ % ^, parentheses, unary
 * minus and decimals. Shared by the calculator UI and its agent tools.
 */

type Token = { kind: 'num'; value: number } | { kind: 'op'; value: string } | { kind: 'paren'; value: '(' | ')' }

const PRECEDENCE: Record<string, number> = { '+': 1, '-': 1, '*': 2, '/': 2, '%': 2, '^': 3, 'neg': 4 }
const RIGHT_ASSOC = new Set(['^', 'neg'])

function tokenize(input: string): Token[] {
  const src = input.replace(/×/g, '*').replace(/÷/g, '/').replace(/−/g, '-').replace(/\s+/g, '')
  const tokens: Token[] = []
  let i = 0
  while (i < src.length) {
    const ch = src[i]
    if (/[0-9.]/.test(ch)) {
      let j = i
      while (j < src.length && /[0-9.]/.test(src[j])) j++
      const text = src.slice(i, j)
      if ((text.match(/\./g) ?? []).length > 1) throw new Error(`Invalid number "${text}"`)
      tokens.push({ kind: 'num', value: Number(text) })
      i = j
    } else if ('+-*/%^'.includes(ch)) {
      const prev = tokens[tokens.length - 1]
      const unary = ch === '-' && (!prev || prev.kind === 'op' || (prev.kind === 'paren' && prev.value === '('))
      tokens.push({ kind: 'op', value: unary ? 'neg' : ch })
      i++
    } else if (ch === '(' || ch === ')') {
      tokens.push({ kind: 'paren', value: ch })
      i++
    } else {
      throw new Error(`Unexpected character "${ch}"`)
    }
  }
  return tokens
}

function apply(op: string, stack: number[]): void {
  if (op === 'neg') {
    const a = stack.pop()
    if (a === undefined) throw new Error('Malformed expression')
    stack.push(-a)
    return
  }
  const b = stack.pop()
  const a = stack.pop()
  if (a === undefined || b === undefined) throw new Error('Malformed expression')
  switch (op) {
    case '+': stack.push(a + b); break
    case '-': stack.push(a - b); break
    case '*': stack.push(a * b); break
    case '/':
      if (b === 0) throw new Error('Division by zero')
      stack.push(a / b)
      break
    case '%': stack.push(a % b); break
    case '^': stack.push(a ** b); break
  }
}

/** Evaluate an arithmetic expression; throws a descriptive Error when invalid. */
export function evaluate(expression: string): number {
  const tokens = tokenize(expression)
  if (tokens.length === 0) throw new Error('Empty expression')
  const values: number[] = []
  const ops: string[] = []
  for (const t of tokens) {
    if (t.kind === 'num') values.push(t.value)
    else if (t.kind === 'paren' && t.value === '(') ops.push('(')
    else if (t.kind === 'paren') {
      while (ops.length && ops[ops.length - 1] !== '(') apply(ops.pop()!, values)
      if (ops.pop() !== '(') throw new Error('Mismatched parentheses')
    } else {
      const p = PRECEDENCE[t.value]
      while (ops.length) {
        const top = ops[ops.length - 1]
        if (top === '(') break
        const q = PRECEDENCE[top]
        if (q > p || (q === p && !RIGHT_ASSOC.has(t.value))) apply(ops.pop()!, values)
        else break
      }
      ops.push(t.value)
    }
  }
  while (ops.length) {
    const op = ops.pop()!
    if (op === '(') throw new Error('Mismatched parentheses')
    apply(op, values)
  }
  if (values.length !== 1 || !Number.isFinite(values[0])) throw new Error('Malformed expression')
  // Trim floating-point noise (0.1 + 0.2 → 0.3).
  return Number.parseFloat(values[0].toPrecision(12))
}

/** Format a result for the display: grouping, at most 9 significant digits. */
export function formatDisplay(value: number): string {
  if (!Number.isFinite(value)) return 'Error'
  if (Math.abs(value) >= 1e9 || (Math.abs(value) < 1e-6 && value !== 0)) {
    // Up to 9 significant digits, like iPadOS: 9.86666667e11, 1e12.
    const [mantissa, exponent] = value.toExponential(8).split('e')
    return `${mantissa.replace(/\.?0+$/, '')}e${exponent.replace('+', '')}`
  }
  const [int, frac] = String(Number.parseFloat(value.toPrecision(9))).split('.')
  const grouped = Number(int).toLocaleString('en-US')
  return frac ? `${grouped}.${frac}` : grouped
}
