// Pure modules of packages/ui that the kit reuses (aliased in vite.config.ts, so nothing is copied and nothing drifts).
declare module 'kit:icons' {
  export const icons: Readonly<Record<string, string>>
  export type IconName = string
}
declare module 'kit:contrast' {
  export function inkFor(value: unknown): string | undefined
  export function cssColor(value: unknown): string | undefined
}
declare module 'kit:keys' {
  export function moveIndex(
    count: number,
    current: number,
    key: string,
    orientation?: 'vertical' | 'horizontal' | 'both',
    wrap?: boolean
  ): number | null
  export function nextEnabled(disabled: readonly boolean[], from: number, direction: 1 | -1): number
}
declare module 'virtual:kit-tokens.css'
