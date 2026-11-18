//! Layout (spec §1.2): stacks, grid, scroll, containers and client-evaluated structure.

use crate::ui::spec::BindingKind::Scalar;
use crate::ui::spec::{
    Children, ComponentSpec, EventSpec, Group, NameRule, ObjectDef, PropSpec, PropType, SlotScope, SlotSpec,
    Tier,
};

use super::PALETTE;

const COLUMN: ObjectDef = ObjectDef {
    name: "GridColumn",
    fields: &[
        prop!("fixed", PropType::Number).doc("A fixed width in pt."),
        prop!("flex", PropType::Number).doc("A flexible share."),
        prop!("min", PropType::Number),
        prop!("max", PropType::Number),
    ],
    open: false,
};

const GRID_TRACKS: PropType = PropType::Union(&[PropType::Int, PropType::List(&PropType::Object(&COLUMN))]);

const SCROLL_POSITION: ObjectDef = ObjectDef {
    name: "ScrollPosition",
    fields: &[prop!("x", PropType::Number), prop!("y", PropType::Number)],
    open: false,
};

pub const STACK: ComponentSpec = ComponentSpec::new(
    "Stack",
    Group::Layout,
    Tier::One,
    "Lays children out along one axis. `VStack` and `HStack` are `axis` sugar in the Lua constructors.",
)
.props(props![
    prop!("axis", en!("v", "h")).default("\"v\""),
    prop!("spacing", PropType::Spacing),
    prop!("align", en!("start", "center", "end", "stretch")),
    prop!("justify", en!("start", "center", "end", "between", "around")),
    prop!("wrap", PropType::Bool).doc("Horizontal stacks only: flow onto new lines."),
])
.children(Children::Any)
.a11y("", NameRule::None);

pub const ZSTACK: ComponentSpec =
    ComponentSpec::new("ZStack", Group::Layout, Tier::One, "Overlays children, back to front.")
        .props(props![prop!(
            "align",
            en!(
                "topLeading",
                "top",
                "topTrailing",
                "leading",
                "center",
                "trailing",
                "bottomLeading",
                "bottom",
                "bottomTrailing"
            )
        )])
        .children(Children::Any);

pub const GRID: ComponentSpec =
    ComponentSpec::new("Grid", Group::Layout, Tier::One, "A grid of equally sized cells.")
        .props(props![
            PropSpec::new("columns", GRID_TRACKS).doc("A count, or a track list."),
            prop!("rows", GRID_TRACKS),
            prop!("spacing", PropType::Spacing),
            prop!("rowSpacing", PropType::Spacing),
            prop!("colSpacing", PropType::Spacing),
            prop!("cellAspect", PropType::Number).doc("Cell width / height."),
        ])
        .children(Children::Any);

pub const SCROLL_VIEW: ComponentSpec =
    ComponentSpec::new("ScrollView", Group::Layout, Tier::One, "Scrolls its content.")
        .props(props![
            prop!("axis", en!("v", "h", "both")).default("\"v\""),
            prop!("indicators", PropType::Bool),
            prop!("scrollTo", PropType::Str).bind(Scalar).doc("The key to scroll to (client state)."),
            prop!("offset", PropType::Object(&SCROLL_POSITION)).bind(Scalar).doc("Out-binding, throttled."),
            prop!("snap", PropType::Bool),
            prop!("refresh", PropType::Bool).doc("Enables pull to refresh."),
        ])
        .events(events![
            EventSpec::new("scroll")
                .payload(&[prop!("x", PropType::Number).req(), prop!("y", PropType::Number).req()])
                .doc("Throttled."),
            EventSpec::new("refresh"),
        ])
        .children(Children::Any);

pub const SPACER: ComponentSpec =
    ComponentSpec::new("Spacer", Group::Layout, Tier::One, "Flexible empty space.")
        .props(props![prop!("min", PropType::Number)]);

pub const DIVIDER: ComponentSpec =
    ComponentSpec::new("Divider", Group::Layout, Tier::One, "A separator line.")
        .a11y("separator", NameRule::None);

pub const GROUP: ComponentSpec = ComponentSpec::new(
    "Group",
    Group::Layout,
    Tier::One,
    "A transparent container with no layout. Holds presentation nodes and passes everything through.",
)
.children(Children::Any);

pub const ADAPTIVE: ComponentSpec = ComponentSpec::new(
    "Adaptive",
    Group::Layout,
    Tier::One,
    "Chooses `compact` or `regular` on the client by container width, so resizing never round-trips.",
)
.props(props![prop!("breakpoint", PropType::Number).default("560").doc("Container width in px.")])
.slots(slots![SlotSpec::new("compact").req(), SlotSpec::new("regular").req()]);

pub const SCOPE: ComponentSpec = ComponentSpec::new(
    "Scope",
    Group::Layout,
    Tier::One,
    "Re-points the colour scheme, tint and background for a subtree: the instrument canvas (a Calculator's black body).",
)
.props(props![prop!("scheme", en!("light", "dark")), prop!("tint", PropType::Enum(&PALETTE))])
.children(Children::Any);

pub const SHOW: ComponentSpec = ComponentSpec::new(
    "Show",
    Group::Layout,
    Tier::One,
    "Client-evaluated conditional: the children when `when` is truthy, else `otherwise`.",
)
.props(props![prop!("when", PropType::Bool).req()])
.slots(slots![SlotSpec::new("otherwise")])
.children(Children::Any);

pub const FOR_EACH: ComponentSpec = ComponentSpec::new(
    "ForEach",
    Group::Layout,
    Tier::One,
    "Repeats a row template once per row of a source. The template is sent once; rows travel as data.",
)
.props(props![
    prop!("source", PropType::Source).req(),
    prop!("key", PropType::Str).doc("The row field used as the key (default: the source key)."),
])
.slots(slots![SlotSpec::new("row")
    .one()
    .req()
    .scope(SlotScope::Row)
    .doc("The row template; `row.*` refs are in scope.")]);

pub const CARD: ComponentSpec =
    ComponentSpec::new("Card", Group::Layout, Tier::One, "A surface that groups content.")
        .props(props![
            prop!("size", en!("small", "medium", "large", "auto")),
            prop!("surface", en!("solid", "material")),
        ])
        .children(Children::Any);
