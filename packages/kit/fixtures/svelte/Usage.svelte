<!-- Checked by scripts/kit-types.test.ts (svelte-check) against the generated Svelte types: what a Svelte app writes must
     type-check. Wrong.svelte holds what must not. -->
<script lang="ts">
  let checked = $state(false)
</script>

<ui-list label="Tasks" selection="single" variant="sidebar" data-testid="tasks">
  <ui-section header="Today">
    <ui-row heading="Buy milk" subheading="Two litres" icon="bell" icon-color="red" tappable selected onclick={() => {}} />
  </ui-section>
  <ui-button variant="tinted" size="small" shape="capsule" fill="orange" onclick={() => {}} data-testid="add">Add</ui-button>
  <ui-toggle {checked} label="Sync" onchange={(e) => (checked = e.detail.checked)} />
  <ui-segmented label="Filter" value="a" options={[{ value: 'a', label: 'A' }]} onchange={(e) => e.detail.value.toUpperCase()} />
  <ui-text-field label="Name" type="email" oninput={(e) => e.data} onchange={() => {}} />
  <ui-banner tone="warning" heading="Low storage" />
</ui-list>
