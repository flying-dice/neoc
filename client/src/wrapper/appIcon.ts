/**
 * The icon of an app's own page (`GET /apps/<id>`): what a browser tab shows, and what "Add to Home Screen" puts on a
 * phone. It is the app's icon as the Dock draws it (its symbol in white on its tint), drawn here as a picture: a home
 * screen takes a PNG, the host has only the symbol's name, and so the page draws the symbol on a canvas and names the
 * result as its icon. Nothing is fetched: the picture is a `data:` URL, which the page's policy allows for images.
 */
import { icons } from '@vos/ui'
import { iconOf } from '../shell/hostApi'

/** The tints an app may declare, as colours (the light values of the kit's palette: a home screen has no dark icons). */
const TINTS: Record<string, string> = {
  red: '#d70015',
  orange: '#ff9500',
  yellow: '#ffcc00',
  green: '#34c759',
  mint: '#00c7be',
  teal: '#30b0c7',
  cyan: '#32ade6',
  blue: '#0066cc',
  indigo: '#5856d6',
  purple: '#af52de',
  pink: '#ff2d55',
  gray: '#8e8e93'
}

/** The side of the picture in pixels: large enough for any home screen (iOS asks for 180). */
export const ICON_SIZE = 512

/** `colour` mixed with white, `share` of it the colour (`#rrggbb` in and out). */
export function lighten(colour: string, share: number): string {
  const channel = (at: number): string => {
    const value = parseInt(colour.slice(at, at + 2), 16)
    return Math.round(value * share + 255 * (1 - share))
      .toString(16)
      .padStart(2, '0')
  }
  return `#${channel(1)}${channel(3)}${channel(5)}`
}

/**
 * The icon as an SVG document: a full square of the tint (lighter at the top, as in the Dock; the home screen rounds
 * the corners itself) with the symbol in the middle half. An unknown symbol or tint is the generic one, as in the shell.
 */
export function appIconSvg(icon: string | undefined, tint: string | undefined): string {
  const colour = TINTS[tint ?? ''] ?? TINTS.blue!
  const half = ICON_SIZE / 2
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" width="${ICON_SIZE}" height="${ICON_SIZE}" viewBox="0 0 ${ICON_SIZE} ${ICON_SIZE}">` +
    `<defs><linearGradient id="tint" x1="0" y1="0" x2="0" y2="1">` +
    `<stop offset="0" stop-color="${lighten(colour, 0.78)}"/><stop offset="1" stop-color="${colour}"/>` +
    `</linearGradient></defs>` +
    `<rect width="${ICON_SIZE}" height="${ICON_SIZE}" fill="url(#tint)"/>` +
    // the symbols are drawn on a 24-unit grid with a 2-unit stroke
    `<g transform="translate(${half / 2} ${half / 2}) scale(${half / 24})" fill="none" stroke="#fff" color="#fff" stroke-width="2" ` +
    `stroke-linecap="round" stroke-linejoin="round">${icons[iconOf(icon)]}</g></svg>`
  )
}

/** The SVG drawn as a PNG `data:` URL, or `null` where the page cannot draw (no canvas, a picture that does not load). */
async function asPng(svg: string, root: Document): Promise<string | null> {
  const image = new Image()
  image.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`
  try {
    await image.decode()
  } catch {
    return null
  }
  const canvas = root.createElement('canvas')
  canvas.width = ICON_SIZE
  canvas.height = ICON_SIZE
  const context = canvas.getContext('2d')
  if (context === null) return null
  context.drawImage(image, 0, 0, ICON_SIZE, ICON_SIZE)
  return canvas.toDataURL('image/png')
}

/** Give the page the app's icon: its tab icon and its home-screen icon. A page that cannot draw keeps the browser's. */
export async function showAppIcon(
  root: Document,
  icon: string | undefined,
  tint: string | undefined
): Promise<void> {
  const png = await asPng(appIconSvg(icon, tint), root)
  if (png === null) return
  for (const rel of ['icon', 'apple-touch-icon']) {
    const link = root.createElement('link')
    link.rel = rel
    link.href = png
    if (rel === 'icon') link.type = 'image/png'
    root.head.append(link)
  }
}
