export type Operator = '+' | '-' | '*' | '/'

export type KeyAction =
  | { type: 'digit'; value: string }
  | { type: 'operator'; value: Operator }
  | { type: 'equals' }
  | { type: 'clear' }
  | { type: 'negate' }
  | { type: 'percent' }
  | { type: 'backspace' }
