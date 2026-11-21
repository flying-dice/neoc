//! Collections (spec §1.4): lists, sections, rows, forms.

use crate::ui::spec::BindingKind::{Collection, Scalar};
use crate::ui::spec::{
    Children, ComponentSpec, EventSpec, Group, NameRule, PropSpec, PropType, SlotScope, SlotSpec, Tier,
};

use super::PALETTE;

pub const LIST: ComponentSpec = ComponentSpec::new(
    "List",
    Group::Collections,
    Tier::One,
    "A scrolling list of rows. `source` + `row` is shorthand for a single `ForEach` child.",
)
.props(props![
    prop!("style", en!("plain", "inset", "insetGrouped", "grouped", "sidebar")).default("\"plain\""),
    prop!("source", PropType::Source),
    prop!("selection", PropType::Json).bind(Collection).doc("The selected row key, or a list of keys."),
    prop!("selectionMode", en!("none", "single", "multi")),
    prop!("editing", PropType::Bool).bind(Scalar),
    prop!("rowHeight", en!("fixed", "auto")),
    prop!("separators", PropType::Bool),
    prop!("groupBy", PropType::Str).doc("A source field: rows are grouped under section headers."),
    prop!("hasMore", PropType::Bool),
    prop!("anchor", en!("start", "end")).doc("`end` for chat-style lists that stick to the bottom."),
])
.events(events![
    EventSpec::new("tap").payload(&[PropSpec::new("key", PropType::Json).req()]),
    EventSpec::new("move")
        .payload(&[PropSpec::new("key", PropType::Json).req(), PropSpec::new("beforeKey", PropType::Json)]),
    EventSpec::new("delete").payload(&[PropSpec::new("key", PropType::Json).req()]),
    EventSpec::new("refresh").doc("The client shows a spinner until the reply."),
    EventSpec::new("loadMore"),
    EventSpec::new("selectionChange").payload(&[PropSpec::new("keys", PropType::Json).req()]),
])
.slots(slots![
    SlotSpec::new("row").one().scope(SlotScope::Row).doc("The row template for `source`."),
    SlotSpec::new("header"),
    SlotSpec::new("footer"),
    SlotSpec::new("empty"),
])
.children(Children::Any)
.interactive()
.a11y("list", NameRule::None);

pub const SECTION: ComponentSpec =
    ComponentSpec::new("Section", Group::Collections, Tier::One, "A group of rows with a header and footer.")
        .props(props![
            prop!("header", PropType::Str),
            prop!("footer", PropType::Str),
            prop!("collapsible", PropType::Bool),
            prop!("expanded", PropType::Bool).bind(Scalar),
        ])
        .slots(slots![SlotSpec::new("header"), SlotSpec::new("footer")])
        .children(Children::Any);

pub const ROW: ComponentSpec = ComponentSpec::new(
    "Row",
    Group::Collections,
    Tier::One,
    "A list row: a title and subtitle, or custom content in the default slot.",
)
.props(props![
    prop!("title", PropType::Str),
    prop!("subtitle", PropType::Str),
    prop!("value", PropType::Str).doc("Trailing secondary text."),
    prop!("icon", PropType::Str),
    prop!("iconTint", PropType::Enum(&PALETTE)),
    prop!("badge", PropType::Union(&[PropType::Str, PropType::Number])),
    prop!("chevron", PropType::Bool),
    prop!("role", en!("normal", "destructive", "action")),
])
.events(events![EventSpec::new("tap")])
.slots(slots![
    SlotSpec::new("leading"),
    SlotSpec::new("trailing"),
    SlotSpec::new("swipeLeading").doc("Buttons revealed by swiping right."),
    SlotSpec::new("swipeTrailing").doc("Buttons revealed by swiping left."),
])
.children(Children::Any)
.positional("title")
.a11y("listitem", NameRule::OneOf { props: &["title", "label"], children: true });

pub const FORM: ComponentSpec = ComponentSpec::new(
    "Form",
    Group::Collections,
    Tier::One,
    "An inset-grouped list of control rows (sugar for `List` with `insetGrouped`).",
)
.children(Children::Any)
.a11y("form", NameRule::None);

pub const LABELED_CONTENT: ComponentSpec = ComponentSpec::new(
    "LabeledContent",
    Group::Collections,
    Tier::One,
    "A label with a value or content beside it.",
)
.props(props![prop!("value", PropType::Str)])
.children(Children::Any)
.a11y("", NameRule::OneOf { props: &["label"], children: false });

pub const DISCLOSURE_GROUP: ComponentSpec = ComponentSpec::new(
    "DisclosureGroup",
    Group::Collections,
    Tier::One,
    "Content that expands and collapses under a label.",
)
.props(props![prop!("expanded", PropType::Bool).bind(Scalar)])
.slots(slots![SlotSpec::new("label").one()])
.children(Children::Any)
.a11y("group", NameRule::OneOf { props: &["label"], children: false });
