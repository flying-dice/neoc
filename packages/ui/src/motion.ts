/**
 * Svelte transitions run in JS, so the `prefers-reduced-motion` rule in
 * base.css doesn't reach them. Wrap every transition duration in `motion()`.
 *
 *   transition:fade={{ duration: motion(220) }}
 */
export function motion(ms: number): number {
  const reduce = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches
  return reduce ? 0 : ms
}
