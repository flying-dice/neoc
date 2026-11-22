<!-- @molecule IconButton — a plain, square Button with an SF-style glyph. Atoms: Button, Icon. Test id: `testid`. -->
<script lang="ts">
  import Button from '../atoms/Button.svelte'
  import Icon from '../atoms/Icon.svelte'
  import type { IconName } from '../icons'

  interface Props {
    testid: string
    icon: IconName
    /** The accessible name: the glyph says nothing to a screen reader. */
    label: string
  }

  let { testid, icon, label }: Props = $props()
</script>

<Button {testid} variant="plain" square aria-label={label}>
  <Icon name={icon} size={19} weight={2.2} />
</Button>
