<!-- Checked by scripts/kit-types.test.ts (svelte-check) against the generated Svelte types: what a Svelte app writes must
     type-check. Wrong.svelte holds what must not. -->
<script lang="ts">
  let checked = $state(false)
</script>

<ui-list label="Tasks" selection="single" variant="sidebar" data-testid="tasks">
  <ui-section header="Today">
    <ui-row heading="Buy milk" subheading="Two litres" icon="bell" icon-color="red" tappable selected onpress={(e) => e.type} />
  </ui-section>
  <ui-button variant="tinted" size="small" shape="capsule" fill="orange" hit onclick={() => {}} data-testid="add">Add</ui-button>
  <ui-toggle {checked} hit label="Sync" onchange={(e) => (checked = e.detail.checked)} />
  <ui-segmented hit label="Filter" value="a" options={[{ value: 'a', label: 'A' }]} onchange={(e) => e.detail.value.toUpperCase()} />
  <ui-text-field hit label="Name" type="email" oninput={(e) => e.data} onchange={() => {}} />
  <ui-banner tone="warning" heading="Low storage" />
</ui-list>
<ui-split-view column="detail" oncollapsedchange={(e) => (checked = e.detail.collapsed)}>
  <div slot="sidebar"></div>
  <div slot="content"></div>
  <div>
    <ui-text-area label="Notes" variant="bare" rows={4} maxlength={200} readonly oninput={(e) => e.data} onchange={() => {}} />
    <ui-picker hit label="Folder" value="1" options={[{ value: '1', label: 'Notes' }]} onchange={(e) => e.detail.value.toUpperCase()} />
  </div>
</ui-split-view>
<ui-alert open={checked} destructive heading="Delete Work?" message="Its todos go too." action-label="Delete" onconfirm={(e) => e.type} oncancel={() => {}} />
