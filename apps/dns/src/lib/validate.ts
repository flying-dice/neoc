/**
 * What is wrong with what is typed, said at the field it is typed in. The backend checks the same things
 * (server/dnsmasq.luau) and has the last word; this is so that a person hears of a slip where they made it, before
 * anything is sent. Each function answers with a sentence, or with nothing when there is nothing wrong (an empty
 * field is not wrong yet: there is nothing to add).
 */
import { portOf, upstreamsOf } from './format'

const V4 = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/
const GROUP = /^[0-9a-f]{1,4}$/

const isV4 = (text: string): boolean => {
  const parts = V4.exec(text)
  return parts !== null && parts.slice(1).every((part) => Number(part) <= 255)
}

/** How many groups of one to four hex digits `text` is, joined by colons; nothing when it is something else. */
const groups = (text: string): number | undefined => {
  if (text === '') return 0
  const parts = text.split(':')
  return parts.every((part) => GROUP.test(part)) ? parts.length : undefined
}

const isV6 = (text: string): boolean => {
  const gap = text.indexOf('::')
  if (gap < 0) return groups(text) === 8
  const before = groups(text.slice(0, gap))
  const after = groups(text.slice(gap + 2))
  // `::` stands for at least one group of zeros
  return before !== undefined && after !== undefined && before + after <= 7
}

export const isAddress = (typed: string): boolean => {
  const text = typed.trim().toLowerCase()
  return isV4(text) || isV6(text)
}

/** What is wrong with a name as it is typed. */
export function nameProblem(typed: string): string | null {
  const text = typed.trim().toLowerCase().replace(/\.$/, '')
  if (text === '') return null
  if (isV4(text)) return 'This is an address: it goes in the other field.'
  if (text.length > 253) return 'A name is 253 characters at most.'
  if (/[^a-z0-9.-]/.test(text)) return 'Only letters, digits, hyphens and dots, like nas.home.'
  const labels = text.split('.')
  if (labels.some((label) => label === '')) return 'There is nothing between two of its dots.'
  if (labels.some((label) => label.length > 63)) return 'A part between dots is 63 characters at most.'
  if (labels.some((label) => label.startsWith('-') || label.endsWith('-'))) return 'A part does not start or end with a hyphen.'
  return null
}

/** What is wrong with an address as it is typed. */
export function addressProblem(typed: string): string | null {
  return typed.trim() === '' || isAddress(typed) ? null : 'Not an IP address, like 192.168.1.10 or fd00::10.'
}

/** What is wrong with the port as it is typed. Empty is wrong here: the server answers on some port. */
export function portProblem(typed: string): string | null {
  return portOf(typed) === undefined ? 'A number from 1 to 65535.' : null
}

/** What is wrong with the upstream servers as they are typed: the first that is not an address. None at all is fine. */
export function upstreamsProblem(typed: string): string | null {
  const servers = upstreamsOf(typed)
  const wrong = servers.find((server) => !isAddress(server))
  if (wrong !== undefined) return `${wrong} is not an IP address, like 1.1.1.1 or 2606:4700:4700::1111.`
  return new Set(servers.map((server) => server.toLowerCase())).size > 8 ? 'Eight servers at most.' : null
}
