<!--
  @molecule Avatar — a person or agent: an image, or initials on a colour (`tint`: a palette token or `#rrggbb`;
  the initials' colour is picked for 4.5:1, and with no tint the colour comes from the name). `contact` is an id
  the host resolves to a name; until it does, the id stands in. `status` adds a dot that differs by shape as well
  as colour. The accessible name is the name (or `label`) plus the status. Atoms: Avatar.
-->
<script lang="ts">
  import { Avatar } from '@agentos/ui/atoms'
  import { useView } from '../context'
  import { commonOf, oneOf, str } from '../props'
  import type { AdapterProps } from '../registry'
  import { imageSource } from '../kit/image'
  import { optStr } from '../kit/support'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const p = (name: string): unknown => ctx.view.evaluate(node.props?.[name], scope)

  const common = $derived(commonOf(ctx, node, path, scope))
  const name = $derived(str(p('name')) || str(p('contact')))
  const status = $derived(
    p('status') === undefined
      ? undefined
      : oneOf(p('status'), ['online', 'busy', 'away', 'offline'] as const, 'offline')
  )
</script>

<span class="avatar" style={common.style} title={common.help}>
  <Avatar
    testid={common.testid}
    {name}
    image={imageSource(p('image'))}
    size={oneOf(p('size'), ['xs', 'sm', 'md', 'lg', 'xl'] as const, 'md')}
    {status}
    tint={optStr(p('tint'))}
    label={common.label}
  />
</span>

<style>
  .avatar {
    display: inline-flex;
    flex: none;
  }
</style>
