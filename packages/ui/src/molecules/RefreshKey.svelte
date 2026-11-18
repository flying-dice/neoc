<!--
  @molecule RefreshKey — the keyboard and screen reader way to pull to refresh: a "Refresh" button that is
  invisible until it has focus, then the first thing on the screen. Test id: `testid`.
-->
<script lang="ts">
  interface Props {
    testid: string
    disabled?: boolean
    onclick: () => void
  }

  let { testid, disabled = false, onclick }: Props = $props()
</script>

<button type="button" class="refresh-key" data-testid={testid} {disabled} {onclick}>Refresh</button>

<style>
  .refresh-key {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    padding: 0;
    border: 0;
  }
  .refresh-key:focus-visible {
    position: sticky;
    top: 4px;
    z-index: 30;
    display: block;
    width: auto;
    height: auto;
    margin: 4px auto;
    padding: 8px 16px;
    clip-path: none;
    border-radius: 999px;
    background: var(--tint-fill);
    color: #fff;
    font: var(--type-subheadline);
    font-weight: 600;
    outline: 2px solid var(--tint);
    outline-offset: 2px;
  }
</style>
