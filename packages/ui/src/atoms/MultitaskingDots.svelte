<!-- @atom MultitaskingDots — the iPadOS "···" window handle (Stage Manager / Split View). Test id: `testid`. -->
<script lang="ts">
  interface Props {
    testid: string
    active?: boolean
    label?: string
    onclick?: () => void
  }

  let { testid, active = true, label = 'Window options', onclick }: Props = $props()
</script>

<button type="button" class="ios-mt-dots" class:active aria-label={label} data-testid={testid} {onclick}>
  <span></span><span></span><span></span>
</button>

<style>
  .ios-mt-dots {
    appearance: none;
    border: 0;
    background: none;
    display: inline-flex;
    gap: 4px;
    padding: 6px 10px;
    border-radius: 999px;
    cursor: pointer;
  }
  .ios-mt-dots:hover {
    background: var(--fill-tertiary);
  }
  span {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--label-tertiary);
    transition: background-color var(--duration-fast);
  }
  .active span {
    background: var(--label-secondary);
  }
</style>
