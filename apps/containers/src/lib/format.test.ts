import { describe, expect, test } from 'bun:test'
import { container } from './fakeApi'
import { imageLabel, isUp, runningSummary, stackSummary, stackTone, stateLabel, toneOf } from './format'

describe('a state', () => {
  test('reads as going, not going, in between or broken', () => {
    expect(toneOf('running')).toBe('on')
    expect(toneOf('exited')).toBe('off')
    expect(toneOf('created')).toBe('off')
    expect(toneOf('restarting')).toBe('busy')
    expect(toneOf('dead')).toBe('bad')
  })

  test('is said in a word a person uses: an exited container is stopped', () => {
    expect(stateLabel('exited')).toBe('Stopped')
    expect(stateLabel('running')).toBe('Running')
  })

  test('a container that is going, paused or trying to start can be stopped', () => {
    expect(isUp(container('a', 'x', 'running'))).toBe(true)
    expect(isUp(container('a', 'x', 'paused'))).toBe(true)
    expect(isUp(container('a', 'x', 'restarting'))).toBe(true)
    expect(isUp(container('a', 'x', 'exited'))).toBe(false)
    expect(isUp(container('a', 'x', 'created'))).toBe(false)
  })
})

describe('a stack', () => {
  const stack = (running: number, total: number) => ({ name: 's', running, total, folder: '' })

  test('says how much of it is running', () => {
    expect(stackSummary(stack(0, 3))).toBe('Stopped')
    expect(stackSummary(stack(2, 3))).toBe('2 of 3 running')
    expect(stackSummary(stack(3, 3))).toBe('All 3 running')
    expect(stackSummary(stack(1, 1))).toBe('Running')
  })

  test('reads as going only when all of it is', () => {
    expect(stackTone(stack(0, 2))).toBe('off')
    expect(stackTone(stack(1, 2))).toBe('busy')
    expect(stackTone(stack(2, 2))).toBe('on')
  })
})

test('an untagged image is shown as the start of its digest', () => {
  expect(imageLabel(`sha256:${'0123456789ab'.repeat(5)}0123`)).toBe('0123456789ab')
  expect(imageLabel('postgres:16')).toBe('postgres:16')
})

test('a list of containers says how many are running', () => {
  expect(runningSummary([])).toBe('No containers')
  expect(runningSummary([container('a', 'x', 'running')])).toBe('1 of 1 container running')
  expect(runningSummary([container('a', 'x', 'running'), container('b', 'x', 'exited')])).toBe('1 of 2 containers running')
})
