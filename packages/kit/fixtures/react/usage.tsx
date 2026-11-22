// Compiled by scripts/kit-types.test.ts against the generated React types: what a React app writes must type-check, and
// each `@ts-expect-error` line must be an error (a wrong attribute or event name is flagged).
export const valid = (
  <ui-list label="Tasks" selection="single" variant="sidebar" data-testid="tasks">
    <ui-section header="Today">
      <ui-row heading="Buy milk" subheading="Two litres" icon="bell" icon-color="red" tappable selected onClick={() => {}} />
    </ui-section>
    <ui-button variant="tinted" size="small" shape="capsule" fill="orange" onClick={() => {}} data-testid="add">
      Add
    </ui-button>
    <ui-toggle checked label="Sync" onchange={(e) => e.detail.checked satisfies boolean} />
    <ui-segmented label="Filter" value="a" options={[{ value: 'a', label: 'A' }]} onchange={(e) => e.detail.value.toUpperCase()} />
    <ui-text-field label="Name" type="email" oninput={(e) => e.data} onchange={() => {}} />
    <ui-banner tone="warning" heading="Low storage" />
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
  </div>
)
