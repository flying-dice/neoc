/**
 * Server-sent events over `fetch`: `EventSource` cannot POST, and a run is a POST. The parser follows the SSE
 * line rules (LF, CRLF or CR line ends, `:` comments and keep-alives ignored, several `data:` lines joined by
 * newlines, one event per blank line) and copes with a frame split across any number of chunks.
 */

export class SseParser {
  private buffer = ''
  private data: string[] = []
  private sawCr = false

  /** Feed text; returns the `data` payload of every event completed by it. */
  push(chunk: string): string[] {
    // a CR at the end of the last chunk may be half of a CRLF: swallow the LF that follows it
    if (this.sawCr && chunk.startsWith('\n')) chunk = chunk.slice(1)
    this.sawCr = chunk.endsWith('\r')
    this.buffer += chunk
    const out: string[] = []
    let start = 0
    for (let i = 0; i < this.buffer.length; i++) {
      const c = this.buffer[i]
      if (c !== '\n' && c !== '\r') continue
      const line = this.buffer.slice(start, i)
      if (c === '\r' && this.buffer[i + 1] === '\n') i++
      start = i + 1
      const event = this.line(line)
      if (event !== undefined) out.push(event)
    }
    this.buffer = this.buffer.slice(start)
    return out
  }

  /** The stream ended: an event whose blank line never came is still delivered. */
  end(): string[] {
    const out = this.buffer ? this.push('\n') : []
    const rest = this.dispatch()
    if (rest !== undefined) out.push(rest)
    return out
  }

  private line(line: string): string | undefined {
    if (line === '') return this.dispatch()
    if (line.startsWith(':')) return undefined
    const colon = line.indexOf(':')
    const field = colon < 0 ? line : line.slice(0, colon)
    let value = colon < 0 ? '' : line.slice(colon + 1)
    if (value.startsWith(' ')) value = value.slice(1)
    if (field === 'data') this.data.push(value)
    return undefined
  }

  private dispatch(): string | undefined {
    if (this.data.length === 0) return undefined
    const event = this.data.join('\n')
    this.data = []
    return event
  }
}

/** Every event payload of a response body, until it ends (or the request is aborted, which throws). */
export async function* readSse(body: ReadableStream<Uint8Array>): AsyncGenerator<string> {
  const reader = body.getReader()
  const decoder = new TextDecoder()
  const parser = new SseParser()
  try {
    for (;;) {
      const { done, value } = await reader.read()
      if (done) break
      yield* parser.push(decoder.decode(value, { stream: true }))
    }
    yield* parser.push(decoder.decode())
    yield* parser.end()
  } finally {
    reader.releaseLock()
  }
}
