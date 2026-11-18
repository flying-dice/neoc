/**
 * Where an `Image.src` / `Avatar.image` loads from. The wire form is `{asset}`, `{url}` or `{file}`:
 *  - `asset`: a host-bundled asset, served by the host under `/assets/<name>` (names are plain file names);
 *  - `url`: an https URL (the kit refuses anything else);
 *  - `file`: a file the app may read: the host would have to serve it, which it does not yet, so it shows the
 *    placeholder rather than guessing a path.
 * The kit's `safeImageSrc` makes the final decision; this only turns the wire shape into a candidate string.
 */
const ASSET_NAME = /^[A-Za-z0-9][A-Za-z0-9._-]*$/

export function imageSource(src: unknown): string | undefined {
  if (typeof src === 'string') return src // a bare string: Avatar.image
  if (typeof src !== 'object' || src === null || Array.isArray(src)) return undefined
  const s = src as Record<string, unknown>
  if (typeof s.url === 'string') return s.url
  if (typeof s.asset === 'string' && ASSET_NAME.test(s.asset)) return `/assets/${s.asset}`
  return undefined
}
