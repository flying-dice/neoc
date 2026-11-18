import type { DeviceRow } from '../shell/hostApi'
import { messageOf } from '../view/errors'
import { callParsed, deviceListSchema } from '../shell/results'
import { host } from './host.svelte'

/** The paired devices (Settings > Devices): the list, and revoking one. */
class Devices {
  list = $state<DeviceRow[]>([])
  error = $state<string | null>(null)
  loaded = $state(false)
  /** The device a Revoke was asked for: the page confirms before anything happens. */
  asked = $state<DeviceRow | null>(null)

  async load(): Promise<void> {
    try {
      this.list = (await callParsed(host.api, 'host.devices.list', {}, deviceListSchema)).devices
      this.error = null
    } catch (e) {
      this.error = messageOf(e)
    } finally {
      this.loaded = true
    }
  }

  async revoke(device: DeviceRow): Promise<void> {
    try {
      await host.api.call('host.devices.revoke', { id: device.id })
    } catch (e) {
      this.error = messageOf(e)
    }
    await this.load()
  }
}

export const devices = new Devices()
