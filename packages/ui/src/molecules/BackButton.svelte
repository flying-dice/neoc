<!-- @molecule BackButton — navigation-bar "‹ Back" item. Atoms: Icon. Test id: `testid`. -->
<script lang="ts">
  import Icon from '../atoms/Icon.svelte'

  interface Props {
    testid: string
    label?: string
    onclick?: () => void
  }

  let { testid, label = 'Back', onclick }: Props = $props()
</script>

<button type="button" class="ios-back" data-testid={testid} {onclick}>
  <Icon name="chevron.left" size={22} weight={2.6} />
  <span>{label}</span>
</button>

<style>
  .ios-back {
    appearance: none;
    border: 0;
    background: none;
    padding: 0;
    margin-left: -8px;
    display: inline-flex;
    align-items: center;
    color: var(--tint);
    font: var(--type-body);
    letter-spacing: var(--tracking-body);
    cursor: pointer;
    white-space: nowrap;
  }
  .ios-back:active {
    opacity: 0.4;
  }
</style>
