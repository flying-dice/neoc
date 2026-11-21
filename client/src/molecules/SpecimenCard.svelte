<!-- @molecule SpecimenCard — titled card that frames a UI-kit specimen. Atoms: Card, Text. -->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import { Card, Text } from '@vos/ui/atoms'

  interface Props {
    title: string
    caption?: string
    children: Snippet
  }

  let { title, caption, children }: Props = $props()
</script>

<Card padding="18px 20px 20px">
  <div class="head">
    <Text style="headline" as="h3">{title}</Text>
    {#if caption}<Text style="footnote" color="secondary">{caption}</Text>{/if}
  </div>
  <div class="body">{@render children()}</div>
</Card>

<style>
  .head {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: 14px;
  }
  .body {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px;
  }
</style>
