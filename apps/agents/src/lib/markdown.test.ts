import { describe, expect, test } from 'bun:test'
import { markdownOf } from './markdown'

describe('what the agent writes', () => {
  test('is drawn as Markdown: emphasis, code, lists, headings, quotes and tables', () => {
    expect(markdownOf('**NEOC** (key `NEOC`)')).toBe('<p><strong>NEOC</strong> (key <code>NEOC</code>)</p>')
    expect(markdownOf('- one\n  - two')).toContain('<ul>\n<li>one\n<ul>\n<li>two</li>')
    expect(markdownOf('## Plan')).toBe('<h2>Plan</h2>')
    expect(markdownOf('> said')).toContain('<blockquote>')
    expect(markdownOf('~~gone~~')).toBe('<p><del>gone</del></p>')
    expect(markdownOf('- [x] done')).toContain('<input type="checkbox" disabled="" checked="" aria-label="Done" /> done')
    expect(markdownOf('- [ ] to do')).toContain('<input type="checkbox" disabled="" aria-label="Not done" /> to do')
  })

  test('has its code blocks and tables scroll sideways', () => {
    expect(markdownOf('```sh\nls\n```')).toBe('<pre data-scrolls-x tabindex="0"><code class="language-sh">ls\n</code></pre>')
    expect(markdownOf('| a |\n|---|\n| 1 |')).toMatch(/^<div class="table" data-scrolls-x tabindex="0"><table>[\s\S]*<\/table><\/div>$/)
  })

  test('with a code fence not closed yet, as it streams in, runs the block to the end and closes it', () => {
    const html = markdownOf('Run this:\n\n```sh\ncargo test\n')
    expect(html).toBe('<p>Run this:</p>\n<pre data-scrolls-x tabindex="0"><code class="language-sh">cargo test\n</code></pre>\n')
  })
})

describe('HTML in what the agent writes', () => {
  test('is text: a script is never markup', () => {
    expect(markdownOf('<script>alert(1)</script>')).toBe('&lt;script&gt;alert(1)&lt;/script&gt;')
    expect(markdownOf('Hi <script>alert(1)</script> there')).toBe('<p>Hi &lt;script&gt;alert(1)&lt;/script&gt; there</p>')
  })

  test('is text: an element with a handler, an image that runs code, a comment', () => {
    expect(markdownOf('<img src=x onerror=alert(1)>')).toBe('&lt;img src=x onerror=alert(1)&gt;')
    expect(markdownOf('a <b onclick="x()">b</b>')).toBe('<p>a &lt;b onclick=&quot;x()&quot;&gt;b&lt;/b&gt;</p>')
    expect(markdownOf('<!-- hidden -->')).toBe('&lt;!-- hidden --&gt;')
    expect(markdownOf('<iframe src="https://example.com"></iframe>')).not.toContain('<iframe')
  })
})

describe('links in what the agent writes', () => {
  test('to the web open outside the page, and give it nothing of the page', () => {
    expect(markdownOf('[the spec](https://agentclientprotocol.com)')).toBe(
      '<p><a target="_blank" rel="noopener noreferrer" href="https://agentclientprotocol.com">the spec</a></p>'
    )
    expect(markdownOf('see www.example.com')).toContain('rel="noopener noreferrer" href="http://www.example.com"')
  })

  test('with a protocol that can run code keep their words and lose their address', () => {
    for (const address of ['javascript:alert(1)', 'JavaScript:alert(1)', 'vbscript:x', 'data:text/html,<b>hi</b>']) {
      const html = markdownOf(`[click](${address})`)
      expect(html).toContain('href=""')
      expect(html.toLowerCase()).not.toContain('script:')
      expect(html).not.toContain('data:')
    }
    expect(markdownOf('![x](javascript:alert(1))')).toBe('<p><img src="" alt="x" /></p>')
  })

  test('to a footnote stay in the page', () => {
    expect(markdownOf('x[^1]\n\n[^1]: note')).toContain('<a href="#user-content-fn-1"')
  })
})
