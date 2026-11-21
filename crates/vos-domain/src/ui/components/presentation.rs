//! Presentation. Presentation nodes are portal nodes: they render nothing in flow and mount in the
//! window's overlay layer. `Toast` is imperative (`view.command`), `ContextMenu` is the common `contextMenu`
//! slot and `Tooltip` is the common `help` prop, so none of the three is a node type.

use crate::ui::spec::BindingKind::Scalar;
use crate::ui::spec::{
    Children, ComponentSpec, EventSpec, Group, NameRule, ObjectDef, PropSpec, PropType, SlotSpec, Tier,
};

const DISMISS: EventSpec = EventSpec::new("dismiss");

const ALERT_FIELD: ObjectDef = ObjectDef {
    name: "AlertField",
    fields: &[
        PropSpec::new("key", PropType::Str).req(),
        prop!("placeholder", PropType::Str),
        prop!("secure", PropType::Bool),
    ],
    open: false,
};

pub const SHEET: ComponentSpec =
    ComponentSpec::new("Sheet", Group::Presentation, Tier::One, "A modal sheet.")
        .props(props![
            prop!("presented", PropType::Bool).bind(Scalar),
            prop!("item", PropType::Json)
                .bind(Scalar)
                .doc("Presents when set; an alternative to `presented`."),
            prop!("style", en!("form", "page", "fullScreen")),
            prop!("detents", PropType::List(&en!("small", "medium", "large"))),
            prop!("dismissible", PropType::Bool),
        ])
        .events(events![DISMISS])
        .children(Children::Any)
        .a11y("dialog", NameRule::None);

pub const POPOVER: ComponentSpec = ComponentSpec::new(
    "Popover",
    Group::Presentation,
    Tier::One,
    "A popover anchored to a node. Collapses to a sheet on compact widths.",
)
.props(props![
    prop!("presented", PropType::Bool).req().bind(Scalar),
    prop!("anchor", PropType::Str).doc("The key of the anchor node; default the parent."),
    prop!("edge", en!("top", "bottom", "leading", "trailing")),
    prop!("arrow", PropType::Bool),
])
.events(events![DISMISS])
.children(Children::Any)
.a11y("dialog", NameRule::None);

pub const ALERT: ComponentSpec = ComponentSpec::new(
    "Alert",
    Group::Presentation,
    Tier::One,
    "An alert with actions, and optionally prompt fields. Any action dismisses it.",
)
.props(props![
    prop!("presented", PropType::Bool).req().bind(Scalar),
    prop!("title", PropType::Str).req(),
    prop!("message", PropType::Str),
    prop!("fields", PropType::List(&PropType::Object(&ALERT_FIELD))),
])
.events(events![DISMISS])
.slots(slots![SlotSpec::new("actions").doc("Buttons: `role`, preferred.")])
.a11y("alertdialog", NameRule::None);

pub const CONFIRMATION_DIALOG: ComponentSpec = ComponentSpec::new(
    "ConfirmationDialog",
    Group::Presentation,
    Tier::One,
    "A choice among actions: a popover on regular widths, an action sheet on compact.",
)
.props(props![
    prop!("presented", PropType::Bool).req().bind(Scalar),
    prop!("title", PropType::Str).req(),
    prop!("message", PropType::Str),
    prop!("anchor", PropType::Str),
])
.events(events![DISMISS])
.slots(slots![SlotSpec::new("actions")])
.a11y("dialog", NameRule::None);

pub const MENU: ComponentSpec = ComponentSpec::new(
    "Menu",
    Group::Presentation,
    Tier::One,
    "A menu of items: `Button`, `Toggle`, `Divider`, an inline `Picker`, nested `Menu`, `Section`.",
)
.props(props![prop!("title", PropType::Str), prop!("icon", PropType::Str)])
.events(
    events![EventSpec::new("primaryAction").doc("Tapping the menu itself, when it has a primary action.")],
)
.children(Children::Any)
.interactive()
.positional("title")
.a11y("menu", NameRule::OneOf { props: &["title", "label"], children: false });
