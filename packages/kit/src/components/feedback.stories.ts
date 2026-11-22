import type { Meta, StoryObj } from '@storybook/web-components-vite'
import { html } from 'lit'
import { expect, fn, userEvent, within } from 'storybook/test'
import './ui-banner'
import './ui-button'
import './ui-empty-state'
import './ui-spinner'
import './ui-icon'

const meta: Meta = {
  title: 'Feedback/Banner, empty state and spinner',
  parameters: { controls: { disable: true } }
}
export default meta
type Story = StoryObj

export const Banners: Story = {
  render: () =>
    html`<div style="display:grid;gap:12px;width:420px">
      <ui-banner data-testid="banner-info">Your changes are saved on this device.</ui-banner>
      <ui-banner data-testid="banner-success" tone="success" heading="Backed up"
        >The last backup finished a minute ago.</ui-banner
      >
      <ui-banner data-testid="banner-warning" tone="warning" heading="Low storage"
        >Only 2 GB is left on the volume.
        <ui-button slot="action" data-testid="banner-warning-action" variant="plain" size="small"
          >Manage</ui-button
        ></ui-banner
      >
      <ui-banner data-testid="banner-error" tone="error" heading="Could not save"
        >The disk is full, and a long reason that wraps onto a second line of the banner as it
        should.
        <ui-button
          slot="action"
          data-testid="banner-error-action"
          variant="tinted"
          size="small"
          destructive
          >Retry</ui-button
        ></ui-banner
      >
    </div>`
}

export const BannerAction: Story = {
  render: () =>
    html`<div style="width:420px">
      <ui-banner data-testid="banner" tone="warning"
        >Offline.
        <ui-button slot="action" data-testid="banner-action" variant="plain" size="small"
          >Retry</ui-button
        >
      </ui-banner>
    </div>`,
  play: async ({ canvasElement }) => {
    const onClick = fn()
    const action = within(canvasElement).getByTestId('banner-action')
    action.addEventListener('click', onClick)
    await userEvent.click(action)
    await expect(onClick).toHaveBeenCalledTimes(1)
  }
}

export const EmptyStates: Story = {
  render: () =>
    html`<div style="display:grid;gap:24px;width:360px">
      <ui-empty-state
        data-testid="empty"
        icon="bubble.left"
        heading="No chat open"
        message="Pick an agent in the sidebar to start a chat."
      ></ui-empty-state>
      <ui-empty-state
        data-testid="empty-action"
        icon="sparkles"
        heading="No agents"
        message="Add an agent to start a chat."
      >
        <ui-button slot="action" data-testid="empty-action-button" variant="tinted"
          >Add Agent</ui-button
        >
      </ui-empty-state>
      <ui-empty-state data-testid="empty-bare" heading="Nothing here"></ui-empty-state>
    </div>`
}

export const Spinners: Story = {
  render: () =>
    html`<div style="display:flex;gap:24px;align-items:center">
      <ui-spinner data-testid="spinner"></ui-spinner>
      <ui-spinner data-testid="spinner-large" size="large" label="Loading messages"></ui-spinner>
    </div>`
}

export const Icons: Story = {
  render: () =>
    html`<div style="display:flex;gap:16px;align-items:center;color:var(--label)">
      <ui-icon name="sparkles" size="24"></ui-icon>
      <ui-icon name="gear" size="24" weight="1.5"></ui-icon>
      <ui-icon name="trash" size="24" label="Delete" style="color:var(--ios-red)"></ui-icon>
      <ui-icon name="not.a.glyph" size="24"></ui-icon>
    </div>`
}
