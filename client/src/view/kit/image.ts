/**
 * Where an `Image.src` / `Avatar.image` loads from: the wire form `{url}` (an https URL), or a bare string for
 * `Avatar.image`. The kit's `safeImageSrc` makes the final decision; this only turns the wire shape into a candidate.
 */
export function imageSource(src: unknown): string | undefined {
  if (typeof src === 'string') return src // a bare string: Avatar.image
  if (typeof src !== 'object' || src === null || Array.isArray(src)) return undefined
  const url = (src as Record<string, unknown>).url
  return typeof url === 'string' ? url : undefined
}
