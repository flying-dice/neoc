<!--
  @molecule Markdown — Markdown from the app (an agent's message, a note) rendered by the kit as elements: raw HTML
  is shown as text, images are never fetched, links keep their target only for http, https and mailto and open
  through the `openUrl` command (the shell confirms), never by navigating the window. `extensions` narrows the
  features (tables, tasklist, code); `selectable` allows copying. Atoms: Markdown.
-->
<script lang="ts">
  import { Markdown } from '@vos/ui/atoms'
  import { optionsFromExtensions } from '@vos/ui'
  import { useView } from '../context'
  import { commonOf, str } from '../props'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const p = (name: string): unknown => ctx.view.evaluate(node.props?.[name], scope)

  const common = $derived(commonOf(ctx, node, path, scope))
  const text = $derived(str(p('text')))
  const options = $derived(optionsFromExtensions(p('extensions')))
</script>

<div class="markdown" style={common.style} title={common.help}>
  <Markdown
    testid={common.testid}
    {text}
    {options}
    selectable={p('selectable') === true}
    onlink={(url) => ctx.view.openUrl(url)}
  />
</div>

<style>
  .markdown {
    min-width: 0;
  }
</style>
