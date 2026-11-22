import { describe, expect, test } from 'bun:test'
import { addressProblem, isAddress, nameProblem, portProblem, upstreamsProblem } from './validate'

describe('a name', () => {
  test('that is one has nothing wrong with it, in any case and with a dot at its end', () => {
    for (const name of ['nas.home', 'hello.mydomain.net', 'NAS.Home.', 'nas', ' a-b.c ', 'xn--bcher-kva.example', '4chan.org']) {
      expect(nameProblem(name)).toBeNull()
    }
  })

  test('that is empty is not wrong yet', () => {
    expect(nameProblem('  ')).toBeNull()
  })

  test('says what is wrong with it', () => {
    expect(nameProblem('nas_home')).toBe('Only letters, digits, hyphens and dots, like nas.home.')
    expect(nameProblem('nas home')).toBe('Only letters, digits, hyphens and dots, like nas.home.')
    expect(nameProblem('x,1.2.3.4')).toBe('Only letters, digits, hyphens and dots, like nas.home.')
    expect(nameProblem('a..b')).toBe('There is nothing between two of its dots.')
    expect(nameProblem('.home')).toBe('There is nothing between two of its dots.')
    expect(nameProblem('-a.b')).toBe('A part does not start or end with a hyphen.')
    expect(nameProblem(`${'a'.repeat(64)}.home`)).toBe('A part between dots is 63 characters at most.')
    expect(nameProblem(`${'a.'.repeat(127)}a`)).toBe('A name is 253 characters at most.')
    expect(nameProblem('192.168.1.10')).toBe('This is an address: it goes in the other field.')
  })
})

describe('an address', () => {
  test('is IPv4 or IPv6', () => {
    for (const address of ['192.168.1.10', '0.0.0.1', ' 10.0.0.1 ', 'FD00::10', '::1', '::', '1:2:3:4:5:6:7:8', '2606:4700:4700::1111']) {
      expect(isAddress(address)).toBe(true)
      expect(addressProblem(address)).toBeNull()
    }
  })

  test('is nothing else', () => {
    for (const address of ['256.1.1.1', '1.2.3', '1:2:3:4:5:6:7:8:9', '1::2::3', ':1:2', '12345::1', '1.1.1.1#53', '--server=6.6.6.6', 'upstairs']) {
      expect(isAddress(address)).toBe(false)
      expect(addressProblem(address)).toBe('Not an IP address, like 192.168.1.10 or fd00::10.')
    }
  })

  test('that is empty is not wrong yet', () => {
    expect(addressProblem('')).toBeNull()
  })
})

test('a port is a number from 1 to 65535, and not empty', () => {
  expect(portProblem('53')).toBeNull()
  expect(portProblem('65535')).toBeNull()
  for (const port of ['', '0', '65536', 'dns', '5.3']) expect(portProblem(port)).toBe('A number from 1 to 65535.')
})

test('the upstream servers are addresses, none or up to eight', () => {
  expect(upstreamsProblem('')).toBeNull()
  expect(upstreamsProblem('1.1.1.1, 2606:4700:4700::1111')).toBeNull()
  expect(upstreamsProblem('1.1.1.1, dns.quad9.net')).toBe('dns.quad9.net is not an IP address, like 1.1.1.1 or 2606:4700:4700::1111.')
  expect(upstreamsProblem('1.1.1.1 1.1.1.2 1.1.1.3 1.1.1.4 1.1.1.5 1.1.1.6 1.1.1.7 1.1.1.8 1.1.1.9')).toBe('Eight servers at most.')
})
