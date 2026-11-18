//! Tier 2 and Tier 3 components: names, tiers and placeholder props only (spec §1.4 to §1.8). Their schema is
//! written when the first app that needs them is designed; until then the validator accepts any prop on them.
//! Heavy components own a third-party engine (`CodeMirror`, canvas hit-testing) and are built and gated per app.

use crate::ui::spec::{Children, ComponentSpec, EventSpec, Group, PropSpec, PropType, Tier};

macro_rules! placeholder {
    ($const:ident, $name:literal, $group:expr, $tier:expr, $doc:literal) => {
        pub const $const: ComponentSpec =
            ComponentSpec::new($name, $group, $tier, $doc).open().children(Children::Any);
    };
    ($const:ident, $name:literal, $group:expr, $tier:expr, $doc:literal, props: $props:expr, events: $events:expr) => {
        pub const $const: ComponentSpec = ComponentSpec::new($name, $group, $tier, $doc)
            .open()
            .children(Children::Any)
            .props($props)
            .events($events);
    };
}

placeholder!(
    TABLE,
    "Table",
    Group::Collections,
    Tier::Two,
    "Placeholder. Columns, sortable headers and row selection over a source.",
    props: props![PropSpec::new("source", PropType::Source), PropSpec::new("columns", PropType::Json)],
    events: events![EventSpec::new("sort"), EventSpec::new("tap")]
);
placeholder!(
    LAZY_GRID,
    "LazyGrid",
    Group::Collections,
    Tier::Two,
    "Placeholder. A virtualised grid of items over a source."
);
placeholder!(
    OUTLINE,
    "Outline",
    Group::Collections,
    Tier::Two,
    "Placeholder. A tree of rows with expandable children (Files, Coder)."
);
placeholder!(
    SKELETON,
    "Skeleton",
    Group::Collections,
    Tier::Two,
    "Placeholder. Loading rows that keep the geometry."
);
placeholder!(
    GAUGE,
    "Gauge",
    Group::Display,
    Tier::Two,
    "Placeholder. A value in a range; falls back to `ProgressView` on old clients."
);
placeholder!(CHIP, "Chip", Group::Display, Tier::Two, "Placeholder. A small removable tag.");
placeholder!(
    TOKEN_FIELD,
    "TokenField",
    Group::Controls,
    Tier::Two,
    "Placeholder. A field that holds a list of tokens."
);
placeholder!(
    BANNER,
    "Banner",
    Group::Presentation,
    Tier::Two,
    "Placeholder. An inline message with an optional action."
);
placeholder!(
    LINK,
    "Link",
    Group::Navigation,
    Tier::Two,
    "Placeholder. Opens a URL through the host, with confirmation."
);
placeholder!(
    CHAT_TRANSCRIPT,
    "ChatTranscript",
    Group::Advanced,
    Tier::Two,
    "Placeholder. A chat bound to an AG-UI thread source, with a composer.",
    props: props![PropSpec::new("thread", PropType::Source)],
    events: events![EventSpec::new("send"), EventSpec::new("stop")]
);
placeholder!(
    CALENDAR_GRID,
    "CalendarGrid",
    Group::Advanced,
    Tier::Two,
    "Placeholder. Month, week and day views over event sources."
);
placeholder!(
    PREVIEW,
    "Preview",
    Group::Advanced,
    Tier::Two,
    "Placeholder. A host-rendered preview of a file by mime type."
);
placeholder!(
    CANVAS,
    "Canvas",
    Group::Advanced,
    Tier::Three,
    "Placeholder. A pannable, zoomable drawing surface over shape rows."
);
placeholder!(
    CODE_EDITOR,
    "CodeEditor",
    Group::Advanced,
    Tier::Three,
    "Placeholder. A code editor over a text source."
);
placeholder!(
    DIFF_VIEW,
    "DiffView",
    Group::Advanced,
    Tier::Three,
    "Placeholder. Unified and split diffs with review comments."
);
placeholder!(
    TERMINAL_VIEW,
    "TerminalView",
    Group::Advanced,
    Tier::Three,
    "Placeholder. A terminal over a byte stream source."
);
placeholder!(RATING, "Rating", Group::Controls, Tier::Three, "Placeholder. A star rating.");
placeholder!(CHART, "Chart", Group::Display, Tier::Three, "Placeholder. A chart over a source.");
placeholder!(SPARKLINE, "Sparkline", Group::Display, Tier::Three, "Placeholder. A tiny inline chart.");
placeholder!(
    BREADCRUMB,
    "Breadcrumb",
    Group::Navigation,
    Tier::Three,
    "Placeholder. A path of links (Files)."
);
placeholder!(INSPECTOR, "Inspector", Group::Navigation, Tier::Three, "Placeholder. A trailing detail pane.");

// `WebView` is deferred with the Browser app (open question 5): it has no entry yet.
