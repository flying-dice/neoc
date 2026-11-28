/**
 * A program agent for the journeys: its standard streams relayed to a WebSocket agent, the fake one of e2e/acp-agent.ts.
 * What it reads, a JSON-RPC message a line, goes out a text frame each, and each frame that comes back is written out a
 * line. So a program agent's chats are served by the same fake agent, with its scripts and its record, as a remote one.
 *
 *   bun e2e/acp-relay.ts <ws url> [NAME...]
 *
 * Each environment variable named is sent on the WebSocket's upgrade as an `x-env-<name>` header (lower case, `_` as
 * `-`), so a journey can see what the program was started with. The relay ends when its input ends (the app closed the
 * connection) or the WebSocket closes.
 */
const [url, ...names] = process.argv.slice(2)
if (!url) {
  process.stderr.write('Usage: bun e2e/acp-relay.ts <ws url> [NAME...]\n')
  process.exit(2)
}

const headers = Object.fromEntries(names.map((name) => [`x-env-${name.toLowerCase().replaceAll('_', '-')}`, process.env[name] ?? '']))
// Bun's WebSocket takes the upgrade's headers
const socket = new WebSocket(url, { headers } as unknown as string[])
// what was read before the socket opened
const waiting: string[] = []
let pending = ''

const send = (line: string): void => {
  if (socket.readyState === WebSocket.OPEN) socket.send(line)
  else waiting.push(line)
}

socket.addEventListener('open', () => {
  for (const line of waiting.splice(0)) socket.send(line)
})
socket.addEventListener('message', (event) => {
  process.stdout.write(`${String(event.data)}\n`)
})
socket.addEventListener('close', () => process.exit(0))
socket.addEventListener('error', () => {
  process.stderr.write(`The agent at ${url} could not be reached.\n`)
  process.exit(1)
})

process.stdin.setEncoding('utf8')
process.stdin.on('data', (chunk: string) => {
  pending += chunk
  for (let cut = pending.indexOf('\n'); cut !== -1; cut = pending.indexOf('\n')) {
    const line = pending.slice(0, cut).trim()
    pending = pending.slice(cut + 1)
    if (line !== '') send(line)
  }
})
process.stdin.on('end', () => {
  socket.close()
  // the socket's close ends it; this, when the socket never opened
  setTimeout(() => process.exit(0), 1000)
})
