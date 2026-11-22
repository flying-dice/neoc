/**
 * A random id that works on a plain `http://` LAN address.
 *
 * `crypto.randomUUID` exists only in a secure context (HTTPS or localhost). vos is opened from other devices at
 * `http://<host>:7400`, where it is undefined, so calling it threw and no window could open. `crypto.getRandomValues`
 * is available everywhere, so the id is built from that.
 */
export const randomId = (): string => {
  const bytes = crypto.getRandomValues(new Uint8Array(16))
  // Version 4, variant 1, so the result is still a valid UUID
  bytes[6] = (bytes[6]! & 0x0f) | 0x40
  bytes[8] = (bytes[8]! & 0x3f) | 0x80
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('')
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
}
