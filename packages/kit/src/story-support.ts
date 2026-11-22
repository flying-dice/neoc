import { expect } from 'storybook/test'

/**
 * Touch targets are at least 44 x 44 pt (the design language, `STYLEGUIDE.md` 7): the four corners of a 44 pt box around the
 * centre of the drawn control must all land on it (or on `owner`, the element a part belongs to), so a pseudo-element or
 * padding that widens the hit area counts and a drawn size that is too small does not.
 */
export async function expectHitArea(
  control: HTMLElement,
  owner: HTMLElement = control
): Promise<void> {
  const r = control.getBoundingClientRect()
  const cx = r.left + r.width / 2
  const cy = r.top + r.height / 2
  for (const [dx, dy] of [
    [-21, -21],
    [21, -21],
    [-21, 21],
    [21, 21]
  ] as const) {
    const at = document.elementFromPoint(cx + dx, cy + dy)
    await expect(at, `${control.dataset.testid}: the point ${dx},${dy} from its centre`).toBe(owner)
  }
}
