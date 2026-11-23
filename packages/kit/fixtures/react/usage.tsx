// Compiled by scripts/kit-types.test.ts against the generated React types: what a React app writes must type-check, and
// each `@ts-expect-error` line must be an error (a wrong attribute or event name is flagged).
export const valid = (
  <ui-list label="Tasks" selection="single" variant="sidebar" data-testid="tasks">
    <ui-section header="Today">
      <ui-row heading="Buy milk" subheading="Two litres" icon="bell" icon-color="red" tappable selected onpress={(e) => e.type satisfies string} />
    </ui-section>
    <ui-button variant="tinted" size="small" shape="capsule" fill="orange" hit onClick={() => {}} data-testid="add">
      Add
    </ui-button>
    <ui-toggle checked hit label="Sync" onchange={(e) => e.detail.checked satisfies boolean} />
    <ui-segmented hit label="Filter" value="a" options={[{ value: 'a', label: 'A' }]} onchange={(e) => e.detail.value.toUpperCase()} />
    <ui-text-field hit label="Name" type="email" oninput={(e) => e.data} onchange={() => {}} />
    <ui-banner tone="warning" heading="Low storage" />
    <ui-split-view column="detail" oncollapsedchange={(e) => e.detail.collapsed satisfies boolean}>
      <div slot="sidebar" />
      <ui-text-area label="Notes" variant="bare" rows={4} maxlength={200} readonly oninput={(e) => e.data} onchange={() => {}} />
      <ui-picker hit label="Folder" value="1" options={[{ value: '1', label: 'Notes' }]} onchange={(e) => e.detail.value.toUpperCase()} />
    </ui-split-view>
    <ui-alert open destructive heading="Delete Work?" message="Its todos go too." action-label="Delete" onconfirm={(e) => e.type satisfies string} oncancel={() => {}} />
  </ui-list>
)

export const wrong = (
  <div>
    {/* @ts-expect-error a misspelt attribute */}
    <ui-button varient="tinted" />
    {/* @ts-expect-error a value the variant does not have */}
    <ui-button variant="tinteed" />
    {/* @ts-expect-error React's onChange is not raised by a custom element: the event is onchange */}
    <ui-toggle onChange={() => {}} />
    {/* @ts-expect-error the payload of change is { checked: boolean } */}
    <ui-toggle onchange={(e) => e.detail.value} />
    {/* @ts-expect-error a boolean attribute is a boolean */}
    <ui-toggle checked="yes" />
    {/* @ts-expect-error a split view has no column of that name */}
    <ui-split-view column="middle" />
    {/* @ts-expect-error the payload of a picker's change is { value: string } */}
    <ui-picker onchange={(e) => e.detail.checked} />
  </div>
)
