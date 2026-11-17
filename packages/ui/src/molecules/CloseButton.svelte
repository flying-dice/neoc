<!-- @molecule CloseButton — round gray "xmark" dismiss button used on sheets and windows. Atoms: Icon. Test id: `testid`. -->
<script lang="ts">
  import Icon from '../atoms/Icon.svelte'

  interface Props {
    testid: string
    label?: string
    size?: number
    onclick?: () => void
  }

  let { testid, label = 'Close', size = 30, onclick }: Props = $props()
</script>

<button
  type="button"
  class="ios-close"
  data-testid={testid}
  aria-label={label}
  style:width="{size}px"
  style:height="{size}px"
  {onclick}
>
  <Icon name="xmark" size={Math.round(size * 0.4)} weight={3} />
</button>

<style>
  .ios-close {
    appearance: none;
    border: 0;
    padding: 0;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: var(--fill-tertiary);
    color: var(--label-secondary);
    cursor: pointer;
  }
  .ios-close:hover {
    background: var(--fill-secondary);
  }
  .ios-close:active {
    opacity: 0.6;
  }
</style>
