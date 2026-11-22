/**
 * A workspace with something in every part of it, for the stories: two projects, a board with issues in each column,
 * and pages with pages under them.
 */
import type { Project } from 'app:rpc'
import { WEBSITE, createFakeBackend, issue, page, type FakeBackend } from './fakeApi'

export const APP: Project = { id: 2, key: 'APP', name: 'Mobile App', color: 'purple' }

export const ISSUES = [
  issue(1, 'Write the home page', { type: 'story' }),
  issue(2, 'Fix the footer links', { type: 'bug', priority: 'high', description: 'They go nowhere on the pricing page.' }),
  issue(3, 'Pick the colours', { status: 'doing' }),
  issue(4, 'Compress the images', { priority: 'low' }),
  issue(5, 'Register the domain', { status: 'done' }),
  issue(6, 'Sketch the tab bar', { project: 2, key: 'APP-1' })
]

export const PAGES = [
  page(1, 'Design'),
  page(2, 'Colours', { parent: 1 }),
  page(3, 'Type', { parent: 1 }),
  page(4, 'Launch plan'),
  page(5, 'Roadmap', { project: 2 })
]

export const BODIES = {
  1: 'How the site should look and read.\n\nCalm, plain, and quick to load. Every page says one thing.',
  2: 'Blue for what can be pressed. Gray for what is beside the point.'
}

/** A backend that starts with the sample workspace. */
export const sample = (): FakeBackend => createFakeBackend({ projects: [WEBSITE, APP], issues: ISSUES, pages: PAGES, bodies: BODIES })
