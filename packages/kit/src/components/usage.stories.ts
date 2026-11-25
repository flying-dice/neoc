import type { Meta, StoryObj } from '@storybook/web-components-vite'
import { html } from 'lit'
import { expect, within } from 'storybook/test'
import './ui-list'
import './ui-meter'
import './ui-row'
import './ui-section'
import './ui-tile'
import './ui-value-pair'

const meta: Meta = {
  title: 'Display/Usage',
  tags: ['autodocs'],
  parameters: { controls: { disable: true } }
}
export default meta
type Story = StoryObj

export const Meter: Story = {
  render: () =>
    html`<div style="width:280px;display:flex;flex-direction:column;gap:12px">
      <ui-meter data-testid="meter" label="Memory" value="61"></ui-meter>
      <ui-meter data-testid="meter-empty" label="CPU"></ui-meter>
    </div>`,
  play: async ({ canvasElement }) => {
    const meter = within(canvasElement)
      .getByTestId('meter')
      .shadowRoot!.querySelector('[role="meter"]')!
    await expect(meter.getAttribute('aria-valuenow')).toBe('61')
    await expect(meter.getAttribute('aria-label')).toBe('Memory')
    // a meter with no value is not announced as a reading
    const empty = within(canvasElement).getByTestId('meter-empty').shadowRoot!
    await expect(empty.querySelector('[role="meter"]')).toBeNull()
  }
}

export const Columns: Story = {
  render: () =>
    html`<div style="width:320px;display:flex;gap:8px">
      ${[62, 18, 41, 27].map(
        (load, i) =>
          html`<div style="flex:1;display:flex;flex-direction:column;align-items:center;gap:4px">
            <span class="ios-caption1">${load}%</span>
            <ui-meter orientation="vertical" label=${`Core ${i + 1}`} value=${load}></ui-meter>
            <span class="ios-caption1 ios-secondary">${i + 1}</span>
          </div>`
      )}
    </div>`
}

export const Tile: Story = {
  render: () =>
    html`<div style="width:240px">
      <ui-tile data-testid="tile" heading="Memory">
        <span class="ios-title2">9.8 GB</span
        ><span class="ios-footnote ios-secondary">of 16 GB</span>
        <ui-meter slot="footer" label="Memory" value="61"></ui-meter>
      </ui-tile>
    </div>`
}

export const ValuePairInRows: Story = {
  render: () =>
    html`<ui-list variant="insetGrouped" label="Processes" style="width:420px">
      <ui-section header="Processes">
        <ui-row heading="luau-runtime" subheading="PID 2310 · vos"
          ><ui-value-pair slot="trailing" main="38.2%" secondary="612 MB"></ui-value-pair
        ></ui-row>
        <ui-row heading="sshd" subheading="PID 702 · root"
          ><ui-value-pair slot="trailing" main="—" secondary="14 MB"></ui-value-pair
        ></ui-row>
        <ui-row heading="node" subheading="PID 2874 · vos"
          ><ui-value-pair
            slot="trailing"
            layout="stacked"
            main="2.2%"
            secondary="540 MB"
          ></ui-value-pair
        ></ui-row>
      </ui-section>
    </ui-list>`
}
