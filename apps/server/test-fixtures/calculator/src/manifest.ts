import { defineManifest } from '@agentos/sdk'

export default defineManifest({
  id: 'calculator',
  version: '0.1.0',
  name: 'Calculator',
  icon: 'plus',
  background: 'linear-gradient(180deg, #5a5a5e 0%, #1c1c1e 100%)',
  // Below 480px tall the keys would drop under the 44px minimum hit target.
  window: { width: 340, height: 560, minWidth: 320, minHeight: 480 },
  dock: true
})
