import { describe, expect, test } from 'bun:test'
import { readSse, SseParser } from './sse'

const feed = (parser: SseParser, ...chunks: string[]): string[] =>
  chunks.flatMap((c) => parser.push(c))

describe('SseParser', () => {
  test('one event per blank line', () => {
    expect(feed(new SseParser(), 'data: a\n\ndata: b\n\n')).toEqual(['a', 'b'])
  })

  test('a frame split across chunks, even mid-field and mid-line-ending', () => {
    const p = new SseParser()
    expect(feed(p, 'da', 'ta: {"x"', ':1}\n', '\ndata: two\r', '\n\r', '\n')).toEqual([
      '{"x":1}',
      'two'
    ])
  })

  test('CRLF and bare CR line endings', () => {
    expect(feed(new SseParser(), 'data: a\r\n\r\ndata: b\r\r')).toEqual(['a', 'b'])
  })

  test('comments and keep-alives are ignored, and do not end an event', () => {
    expect(feed(new SseParser(), ': keep-alive\n\ndata: a\n: note\n\n')).toEqual(['a'])
  })

  test('several data lines are one event joined by newlines', () => {
    expect(feed(new SseParser(), 'data: a\ndata: b\n\n')).toEqual(['a\nb'])
  })

  test('only one leading space after the colon is dropped; other fields are ignored', () => {
    expect(feed(new SseParser(), 'event: x\nid: 1\ndata:  two\n\n')).toEqual([' two'])
  })

  test('an event cut off by the end of the stream is still delivered', () => {
    const p = new SseParser()
    expect(feed(p, 'data: tail')).toEqual([])
    expect(p.end()).toEqual(['tail'])
  })
})

describe('readSse', () => {
  const body = (...parts: string[]): ReadableStream<Uint8Array> => {
    const enc = new TextEncoder()
    return new ReadableStream({
      start(c) {
        for (const p of parts) c.enqueue(enc.encode(p))
        c.close()
      }
    })
  }
  const all = async (s: ReadableStream<Uint8Array>): Promise<string[]> => {
    const out: string[] = []
    for await (const e of readSse(s)) out.push(e)
    return out
  }

  test('decodes a multi-byte character split across chunks', async () => {
    const bytes = new TextEncoder().encode('data: héllo\n\n')
    const cut = bytes.indexOf(0xc3) + 1
    const stream = new ReadableStream<Uint8Array>({
      start(c) {
        c.enqueue(bytes.slice(0, cut))
        c.enqueue(bytes.slice(cut))
        c.close()
      }
    })
    expect(await all(stream)).toEqual(['héllo'])
  })

  test('yields every event of a stream', async () => {
    expect(await all(body('data: 1\n\n', 'data: 2\n\n'))).toEqual(['1', '2'])
  })

  test('an errored stream (an abort) rejects the iteration', async () => {
    const stream = new ReadableStream<Uint8Array>({
      start(c) {
        c.enqueue(new TextEncoder().encode('data: 1\n\n'))
        c.error(new DOMException('Aborted', 'AbortError'))
      }
    })
    await expect(all(stream)).rejects.toThrow()
  })
})
