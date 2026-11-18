import { describe, expect, test } from 'bun:test'
import { describeLink } from './link'

describe('describeLink', () => {
  test('a web link names the host of the parsed URL, not the text around it', () => {
    expect(describeLink('https://evil.example/login?next=github.com/acme')).toEqual({
      kind: 'web',
      url: 'https://evil.example/login?next=github.com/acme',
      host: 'evil.example'
    })
  })

  test('userinfo cannot pass as the host', () => {
    expect(describeLink('https://github.com@evil.example/')).toMatchObject({ host: 'evil.example' })
  })

  test('a look-alike domain shows in its ASCII form', () => {
    const link = describeLink('https://аррle.com/')
    expect(link?.kind).toBe('web')
    expect(link?.host.startsWith('xn--')).toBe(true)
  })

  test('the port stays part of the host', () => {
    expect(describeLink('http://localhost:7400/x')).toMatchObject({ host: 'localhost:7400' })
  })

  test('mailto names the recipient', () => {
    expect(describeLink('mailto:team@example.org?subject=Hi')).toEqual({
      kind: 'mail',
      url: 'mailto:team@example.org?subject=Hi',
      host: 'team@example.org'
    })
    expect(describeLink('mailto:a%2Bb@example.org')).toMatchObject({ host: 'a+b@example.org' })
  })

  test.each([
    'javascript:alert(1)',
    'data:text/html,<b>x</b>',
    'file:///etc/passwd',
    'ftp://example.org/',
    '/relative/path',
    'https://',
    'mailto:',
    '',
    '  ',
    'https://example.org/\u0000',
    'https://exa\nmple.org/',
    42,
    null,
    undefined
  ])('%p is not a link a window may open', (href) => {
    expect(describeLink(href)).toBeNull()
  })
})
