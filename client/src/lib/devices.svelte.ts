import type { DeviceRow } from '../shell/hostApi'
import { messageOf } from '../shell/errors'
import { callParsed, deviceListSchema } from '../shell/results'
import { host } from './host.svelte'

/** The paired devices (Settings > Devices): the list, and revoking one. */
export class Devices {
  /** The devices as the host last listed them. */
  list = $state<DeviceRow[]>([])
  /** The host has listed them at least once: an empty `list` before that says nothing. */
  loaded = $state(false)
  /** What went wrong last (the list could not be read, or a device could not be revoked), until a read succeeds. */
  error = $state<string | null>(null)
  /** The device a Revoke was asked for: the page confirms before anything happens. */
  asked = $state<DeviceRow | null>(null)

  async load(): Promise<void> {
    try {
      this.list = (await callParsed(host.api, 'host.devices.list', {}, deviceListSchema)).devices
      this.loaded = true
      this.error = null
    } catch (e) {
      this.error = `The paired devices could not be read: ${messageOf(e)}`
    }
  }

  /** Revoke `device`, then read the list again whatever the host said: it shows what is true now. */
  async revoke(device: DeviceRow): Promise<void> {
    let refused: string | null = null
    try {
      await host.api.call('host.devices.revoke', { id: device.id })
    } catch (e) {
      refused = `${device.name} could not be revoked: ${messageOf(e)}`
    }
    await this.load()
    // said after the read, which clears what was wrong before it: the device is still in the list, and this is why
    if (refused !== null) this.error = refused
  }
}

export const devices = new Devices()
