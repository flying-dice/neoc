//! Display: text, icons, images and read-only indicators.

use crate::ui::spec::{
    Children, ComponentSpec, EventSpec, Group, NameRule, ObjectDef, PropSpec, PropType, SlotSpec, Tier,
};

use super::{LABEL_COLOR, PALETTE, TEXT_STYLE};

const IMAGE_SRC: ObjectDef =
    ObjectDef { name: "ImageSrc", fields: &[prop!("url", PropType::Str).doc("An https URL.")], open: false };

pub const TEXT: ComponentSpec = ComponentSpec::new(
    "Text",
    Group::Display,
    Tier::One,
    "Text in one of the closed text styles. Never `{@html}`: markup is not interpreted.",
)
.props(props![
    PropSpec::new("text", PropType::Str).req().doc("Positional in Lua: `ui.Text { \"Hi\" }`."),
    prop!("style", PropType::Enum(&TEXT_STYLE)).default("\"body\""),
    prop!("weight", en!("regular", "medium", "semibold", "bold")),
    prop!("design", en!("default", "rounded", "mono")),
    prop!("foreground", PropType::Enum(&LABEL_COLOR)),
    prop!("align", en!("leading", "center", "trailing")),
    prop!("lines", PropType::Int).doc("Maximum lines; `0` = unlimited."),
    prop!("truncation", en!("tail", "middle", "head")),
    prop!("selectable", PropType::Bool),
    prop!("digits", en!("tabular")),
    prop!("fit", PropType::Bool).doc("Shrink to the box (`display` style only)."),
    prop!("heading", PropType::Int).doc("Heading level `1..=6`."),
    prop!("live", en!("off", "polite", "assertive")).doc("Announces changes to assistive technology."),
])
.positional("text")
.a11y("", NameRule::None);

pub const LABEL: ComponentSpec =
    ComponentSpec::new("Label", Group::Display, Tier::One, "A title with an icon.")
        .props(props![
            PropSpec::new("title", PropType::Str).req(),
            prop!("icon", PropType::Str),
            prop!("display", en!("titleAndIcon", "titleOnly", "iconOnly")),
        ])
        .positional("title");

pub const ICON: ComponentSpec =
    ComponentSpec::new("Icon", Group::Display, Tier::One, "A glyph from the kit's icon set.")
        .props(props![
            PropSpec::new("name", PropType::Str).req(),
            prop!("size", PropType::Number),
            prop!("weight", en!("regular", "medium", "semibold", "bold")),
        ])
        .positional("name")
        .a11y("img", NameRule::None);

pub const IMAGE: ComponentSpec =
    ComponentSpec::new("Image", Group::Display, Tier::One, "An image. Needs `alt`, or `decorative = true`.")
        .props(props![
            PropSpec::new("src", PropType::Object(&IMAGE_SRC)).req(),
            prop!("fit", en!("fit", "fill")),
            prop!("size", PropType::Number),
            prop!("alt", PropType::Str),
            prop!("decorative", PropType::Bool),
            prop!("placeholder", PropType::Str),
        ])
        .a11y("img", NameRule::AltOrDecorative);

pub const BADGE: ComponentSpec = ComponentSpec::new(
    "Badge",
    Group::Display,
    Tier::One,
    "A count or label; a dot when `value` is omitted.",
)
.props(props![
    prop!("value", PropType::Union(&[PropType::Number, PropType::Str])),
    prop!("max", PropType::Int),
])
.positional("value");

pub const PROGRESS_VIEW: ComponentSpec = ComponentSpec::new(
    "ProgressView",
    Group::Display,
    Tier::One,
    "Progress, determinate (`value` in `0..=1`) or indeterminate (no value).",
)
.props(props![prop!("value", PropType::Number), prop!("style", en!("linear", "circular", "spinner")),])
.a11y("progressbar", NameRule::None);

pub const MARKDOWN: ComponentSpec = ComponentSpec::new(
    "Markdown",
    Group::Display,
    Tier::One,
    "Rendered Markdown (a GFM subset). No raw HTML.",
)
.props(props![
    PropSpec::new("text", PropType::Str).req(),
    prop!("extensions", PropType::List(&en!("tables", "tasklist", "code"))),
    prop!("selectable", PropType::Bool),
])
.positional("text");

pub const AVATAR: ComponentSpec = ComponentSpec::new(
    "Avatar",
    Group::Display,
    Tier::One,
    "A person or agent: initials, an image, or a contact the host resolves.",
)
.props(props![
    prop!("name", PropType::Str),
    prop!("image", PropType::Str),
    prop!("contact", PropType::Str).doc("An agent or contact id, resolved by the host."),
    prop!("size", en!("xs", "sm", "md", "lg", "xl")),
    prop!("status", en!("online", "busy", "away", "offline")),
    prop!("tint", PropType::Enum(&PALETTE)),
])
.a11y("img", NameRule::OneOf { props: &["name", "label"], children: false });

pub const EMPTY_STATE: ComponentSpec = ComponentSpec::new(
    "EmptyState",
    Group::Display,
    Tier::One,
    "What a screen shows when there is nothing to show (the design language requires one).",
)
.props(props![
    prop!("icon", PropType::Str),
    PropSpec::new("title", PropType::Str).req(),
    prop!("message", PropType::Str),
])
.events(events![EventSpec::new("action")])
.slots(slots![SlotSpec::new("action").one().doc("A call-to-action button.")])
.children(Children::None)
.a11y("", NameRule::None);
