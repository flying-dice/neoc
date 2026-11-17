<!-- @molecule AgentAvatar — agent glyph on a coloured tile. Atoms: IconTile, Icon. -->
<script lang="ts">
  import { Icon, IconTile } from '@agentos/ui/atoms'
  import type { IconName } from '@agentos/ui'

  interface Props {
    icon: IconName
    color: string
    size?: number
  }

  let { icon, color, size = 36 }: Props = $props()
</script>

<IconTile {size} background={color}>
  <Icon name={icon} size={Math.round(size * 0.55)} weight={2} color="#fff" />
</IconTile>
