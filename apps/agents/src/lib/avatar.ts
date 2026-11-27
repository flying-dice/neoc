/**
 * An agent's picture: the small square image kept for it (a `data:` URL the page makes from a file the person
 * chose), or, for an agent without one, its initials on a circle of a tint of its own.
 */

/** How many pixels across a picture is kept. */
export const PICTURE_SIZE = 256

/** The kinds of file the form offers to choose from. A GIF is kept as its first frame. */
export const PICTURE_TYPES = 'image/png,image/jpeg,image/webp,image/gif'

/** The tints an agent's initials are drawn on: the palette's colours, without its grays. */
export const TINTS = ['blue', 'red', 'green', 'orange', 'yellow', 'purple', 'pink', 'indigo', 'teal', 'mint', 'cyan', 'brown'] as const

/** The tint of the agent with that id: always the same one for it. */
export function tintOf(id: string): (typeof TINTS)[number] {
  // FNV-1a over the id's UTF-16 code units
  let hash = 0x811c9dc5
  for (let i = 0; i < id.length; i++) hash = Math.imul(hash ^ id.charCodeAt(i), 0x01000193)
  return TINTS[(hash >>> 0) % TINTS.length]!
}

/** The first letter of the name's first two words, in capitals: `Scripted agent` is `SA`, `Writer` is `W`. */
export function initialsOf(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean)
  return words
    .slice(0, 2)
    .map((word) => Array.from(word)[0]!)
    .join('')
    .toLocaleUpperCase()
}

/** The square in the middle of a `width` by `height` picture, the part that is kept: its corner and its side. */
export function squareOf(width: number, height: number): { x: number; y: number; side: number } {
  const side = Math.min(width, height)
  return { x: Math.floor((width - side) / 2), y: Math.floor((height - side) / 2), side }
}

/**
 * The picture kept for a chosen file: the square in its middle, drawn 256 pixels across, as a WebP `data:` URL (a JPEG
 * one in a browser that cannot make WebP: a PNG of a photo this size can be more than the backend keeps). Rejects a
 * file that is not a picture the browser can read.
 */
export async function pictureOf(file: Blob): Promise<string> {
  const bitmap = await createImageBitmap(file)
  try {
    const { x, y, side } = squareOf(bitmap.width, bitmap.height)
    const canvas = document.createElement('canvas')
    canvas.width = PICTURE_SIZE
    canvas.height = PICTURE_SIZE
    const context = canvas.getContext('2d')
    if (!context) throw new Error('This browser cannot draw the picture.')
    context.imageSmoothingQuality = 'high'
    context.drawImage(bitmap, x, y, side, side, 0, 0, PICTURE_SIZE, PICTURE_SIZE)
    // a browser that cannot make the type asked for makes a PNG instead
    const webp = canvas.toDataURL('image/webp', 0.9)
    if (webp.startsWith('data:image/webp;')) return webp
    // a JPEG has no transparency: what was transparent is white, not black
    context.globalCompositeOperation = 'destination-over'
    context.fillStyle = '#ffffff'
    context.fillRect(0, 0, PICTURE_SIZE, PICTURE_SIZE)
    return canvas.toDataURL('image/jpeg', 0.9)
  } finally {
    bitmap.close()
  }
}
