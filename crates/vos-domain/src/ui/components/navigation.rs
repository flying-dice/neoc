//! Navigation (spec §1.3).

use crate::ui::spec::BindingKind::{Collection, Scalar, Text};
use crate::ui::spec::{
    Children, ComponentSpec, EventSpec, Group, NameRule, ObjectDef, PropSpec, PropType, SlotScope, SlotSpec,
    Tier,
};

use super::ROUTE;

const SEARCHABLE: ObjectDef = ObjectDef {
    name: "Searchable",
    fields: &[
        prop!("text", PropType::Str).req().bind(Text).doc("The search text (client state)."),
        prop!("placeholder", PropType::Str),
        prop!("scopes", PropType::List(&PropType::Str)),
        prop!("selectedScope", PropType::Str).bind(Scalar),
        prop!("suggestions", PropType::Source),
        prop!("sync", PropType::Str).doc(super::SYNC_DOC),
    ],
    open: false,
};

pub const SCREEN: ComponentSpec = ComponentSpec::new(
    "Screen",
    Group::Navigation,
    Tier::One,
    "One screen: a navigation bar (title, search, toolbar) over content.",
)
.props(props![
    prop!("title", PropType::Str),
    prop!("subtitle", PropType::Str),
    prop!("titleDisplay", en!("large", "inline", "auto")),
    prop!("backLabel", PropType::Str),
    prop!("searchable", PropType::Object(&SEARCHABLE)).doc("Puts a search field in the navigation bar."),
    prop!("refresh", PropType::Bool),
    prop!("background", en!("plain", "grouped")),
])
.events(events![EventSpec::new("searchSubmit"), EventSpec::new("refresh")])
.slots(slots![
    SlotSpec::new("toolbar.leading"),
    SlotSpec::new("toolbar.trailing"),
    SlotSpec::new("toolbar.principal"),
    SlotSpec::new("toolbar.primaryAction"),
    SlotSpec::new("toolbar.secondaryAction"),
    SlotSpec::new("toolbar.bottom"),
    SlotSpec::new("toolbar.keyboard"),
])
.children(Children::Any)
.a11y("main", NameRule::None);

pub const NAVIGATION_STACK: ComponentSpec = ComponentSpec::new(
    "NavigationStack",
    Group::Navigation,
    Tier::One,
    "A push/pop stack of screens. The path is client state.",
)
.props(props![
    prop!("path", PropType::List(&PropType::Object(&ROUTE))).bind(Collection),
    prop!("rootKey", PropType::Str),
])
.events(events![EventSpec::new("pathChange").payload(&[PropSpec::new("path", PropType::Json).req()])])
.slots(slots![
    SlotSpec::new("root").one().req(),
    SlotSpec::new("dest:")
        .prefix()
        .scope(SlotScope::Route)
        .doc("`dest:<route name>`: a destination template; `route.*` refs are in scope."),
]);

pub const NAVIGATION_LINK: ComponentSpec = ComponentSpec::new(
    "NavigationLink",
    Group::Navigation,
    Tier::One,
    "A row-like control that pushes a route.",
)
.props(props![PropSpec::new("to", PropType::Object(&ROUTE)).req(), prop!("title", PropType::Str)])
.children(Children::Any)
.interactive()
.positional("title")
.a11y("link", NameRule::OneOf { props: &["title", "label"], children: true });

pub const SPLIT_VIEW: ComponentSpec = ComponentSpec::new(
    "SplitView",
    Group::Navigation,
    Tier::One,
    "Two or three columns. Collapses to a stack below 700 px on the client.",
)
.props(props![
    prop!("columns", PropType::Int).doc("Column count: 2 or 3."),
    prop!("visibility", en!("all", "double", "detail")).bind(Scalar),
    prop!("sidebarWidth", en!("narrow", "regular", "wide")),
    prop!("preferredCompact", en!("sidebar", "content", "detail")),
    prop!("style", en!("balanced", "prominentDetail")),
])
.slots(slots![SlotSpec::new("sidebar"), SlotSpec::new("content"), SlotSpec::new("detail")])
.a11y("", NameRule::None);

pub const TAB_VIEW: ComponentSpec = ComponentSpec::new(
    "TabView",
    Group::Navigation,
    Tier::One,
    "Tabs. Each child is a tab; its `tabItem` slot is the tab's label, icon and badge.",
)
.props(props![
    prop!("selection", PropType::Json).bind(Scalar).doc("The `tag` of the selected tab."),
    prop!("style", en!("bottom", "top", "sidebar", "page")),
])
.children(Children::Any)
.a11y("tablist", NameRule::None);
