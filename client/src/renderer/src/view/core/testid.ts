/**
 * Test ids derive from the explicit keys on a node's path, never from index or text (spec 5.4,
 * docs/TESTING.md): `testid = app + "-" + the explicit keys on the path`, "-"-joined.
 *
 * The window root, auto keys (`_0`, given to un-keyed layout wrappers) and slot names are skipped, so wrapping
 * a node in a Stack or moving it into a toolbar slot never renames it. A row template contributes
 * `row-<rowKey>` (the item's stable id). An explicit `testid` prop overrides the derived id.
 *
 *   main/_0/_1/_2/key-7                    calculator-key-7
 *   main/actions/save                      notes-actions-save
 *   main/list/@row/item/delete, row 7      notes-list-row-7-item-delete   (the row container: notes-list-row-7)
 *   main/confirm-delete/@actions/confirm   notes-confirm-delete-confirm
 */
export function testid(app: string, path: string, rowKey?: string | number, override?: unknown): string {
  if (typeof override === 'string' && override) return override
  const out: string[] = []
  for (const seg of path.split('/').slice(1)) {
    if (seg === '@row') {
      out.push('row')
      if (rowKey !== undefined) out.push(String(rowKey))
    } else if (!seg.startsWith('@') && !/^_\d+$/.test(seg)) out.push(seg)
  }
  return [app, ...out].join('-')
}

/** The id of the row container a `ForEach` renders for `rowKey` under the node at `path`. */
export const rowTestid = (app: string, path: string, rowKey: string | number): string => testid(app, `${path}/@row`, rowKey)
