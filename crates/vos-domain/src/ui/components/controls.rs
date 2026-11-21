//! Controls: buttons, toggles, pickers and text input.

use crate::ui::spec::BindingKind::{Scalar, Text};
use crate::ui::spec::{
    Children, ComponentSpec, EventSpec, Group, NameRule, ObjectDef, PropSpec, PropType, SlotSpec, Tier,
};

use super::{PALETTE, SYNC_DOC};

const NAMED: NameRule = NameRule::OneOf { props: &["title", "label"], children: true };
const LABELLED: NameRule = NameRule::OneOf { props: &["label"], children: false };
const TEXT_NAMED: NameRule = NameRule::OneOf { props: &["label", "placeholder"], children: false };

const CHANGE: EventSpec = EventSpec::new("change").payload(&[PropSpec::new("value", PropType::Json).req()]);

const OPTION: ObjectDef = ObjectDef {
    name: "PickerOption",
    fields: &[
        PropSpec::new("value", PropType::Json).req(),
        PropSpec::new("label", PropType::Str).req(),
        prop!("icon", PropType::Str),
        prop!("disabled", PropType::Bool),
    ],
    open: false,
};

const VALIDATION: ObjectDef = ObjectDef {
    name: "Validation",
    fields: &[
        prop!("required", PropType::Bool),
        prop!("pattern", PropType::Str),
        prop!("min", PropType::Number),
        prop!("max", PropType::Number),
        prop!("maxLength", PropType::Int),
    ],
    open: false,
};

const TEXT_KIND: PropType = en!("text", "email", "url", "number", "tel", "search");

pub const SHORTCUT: ComponentSpec = ComponentSpec::new(
    "Shortcut",
    Group::Controls,
    Tier::One,
    "An invisible keyboard command, active while the window is focused and listed in its command list.",
)
.props(props![PropSpec::new("shortcut", PropType::Str).req()])
.events(events![EventSpec::new("trigger")])
.interactive()
.a11y("", NameRule::None);

pub const BUTTON: ComponentSpec =
    ComponentSpec::new("Button", Group::Controls, Tier::One, "A button. Icon-only buttons need a `label`.")
        .props(props![
            prop!("title", PropType::Str),
            prop!("icon", PropType::Str),
            prop!("style", en!("filled", "tinted", "gray", "plain", "bordered")),
            prop!("shape", en!("rounded", "capsule", "circle")),
            prop!("size", en!("small", "medium", "large", "fill")),
            prop!("role", en!("normal", "primary", "destructive", "cancel")),
            prop!("fill", PropType::Enum(&PALETTE))
                .doc("Fill colour; the text colour is chosen for contrast."),
            prop!("selected", PropType::Bool).doc("Pressed state (`aria-pressed`)."),
            prop!("loading", PropType::Bool),
        ])
        .events(events![EventSpec::new("tap"), EventSpec::new("longPress")])
        .children(Children::Any)
        .interactive()
        .positional("title")
        .a11y("button", NAMED);

pub const TOGGLE: ComponentSpec =
    ComponentSpec::new("Toggle", Group::Controls, Tier::One, "A two-state control.")
        .props(props![
            prop!("on", PropType::Bool).req().bind(Scalar),
            prop!("title", PropType::Str),
            prop!("style", en!("switch", "checkbox", "button")),
        ])
        .events(events![EventSpec::new("change").payload(&[PropSpec::new("value", PropType::Bool).req()])])
        .slots(slots![SlotSpec::new("label").one()])
        .children(Children::Max(1))
        .interactive()
        .positional("title")
        .a11y("switch", NAMED);

pub const SLIDER: ComponentSpec =
    ComponentSpec::new("Slider", Group::Controls, Tier::One, "Picks a number in a range.")
        .props(props![
            prop!("value", PropType::Number).req().bind(Scalar),
            prop!("min", PropType::Number).default("0"),
            prop!("max", PropType::Number).default("1"),
            prop!("step", PropType::Number),
            prop!("ticks", PropType::Int),
            prop!("minLabel", PropType::Str),
            prop!("maxLabel", PropType::Str),
        ])
        .events(events![
            EventSpec::new("editingChange").payload(&[PropSpec::new("editing", PropType::Bool).req()])
        ])
        .interactive()
        .a11y("slider", LABELLED);

pub const STEPPER: ComponentSpec =
    ComponentSpec::new("Stepper", Group::Controls, Tier::One, "Increments and decrements a number.")
        .props(props![
            prop!("value", PropType::Number).req().bind(Scalar),
            prop!("min", PropType::Number),
            prop!("max", PropType::Number),
            prop!("step", PropType::Number).default("1"),
            prop!("title", PropType::Str),
        ])
        .events(events![CHANGE])
        .interactive()
        .positional("title")
        .a11y("spinbutton", NAMED);

pub const PICKER: ComponentSpec = ComponentSpec::new(
    "Picker",
    Group::Controls,
    Tier::One,
    "Picks one value (or several) from options. `segmented`, `menu` and `inline` styles are Tier 1; `wheel` is Tier 2.",
)
.props(props![
    prop!("selection", PropType::Json).req().bind(Scalar).doc("The selected value, or a list when `multiple`."),
    prop!("options", PropType::Union(&[PropType::Source, PropType::List(&PropType::Object(&OPTION))])).req(),
    prop!("style", en!("menu", "segmented", "wheel", "inline", "navigation")).default("\"menu\""),
    prop!("multiple", PropType::Bool),
])
.events(events![CHANGE])
.slots(slots![SlotSpec::new("option:").prefix().doc("`option:<value>`: custom option content.")])
.interactive()
.a11y("combobox", LABELLED);

pub const DATE_PICKER: ComponentSpec = ComponentSpec::new(
    "DatePicker",
    Group::Controls,
    Tier::One,
    "Picks a date, a time or both. Values are `YYYY-MM-DD`, `HH:mm` or ISO-8601 UTC.",
)
.props(props![
    prop!("value", PropType::Str).req().bind(Scalar),
    prop!("kind", en!("date", "time", "dateTime")).default("\"date\""),
    prop!("style", en!("compact", "graphical", "wheel")),
    prop!("min", PropType::Str),
    prop!("max", PropType::Str),
    prop!("step", PropType::Int),
])
.events(events![CHANGE])
.interactive()
.a11y("group", LABELLED);

pub const COLOR_PICKER: ComponentSpec =
    ComponentSpec::new("ColorPicker", Group::Controls, Tier::One, "Picks a colour from the palette.")
        .props(props![
            prop!("value", PropType::Str).req().bind(Scalar),
            prop!("palette", PropType::List(&PropType::Str)).doc("Palette tokens offered; default all."),
            prop!("custom", PropType::Bool).doc("Allow any `#rrggbb`."),
            prop!("opacity", PropType::Bool),
        ])
        .events(events![CHANGE])
        .interactive()
        .a11y("group", LABELLED);

pub const TEXT_FIELD: ComponentSpec = ComponentSpec::new(
    "TextField",
    Group::Controls,
    Tier::One,
    "Single- or multi-line text input. The user's text shows at once; the host gets one debounced write.",
)
.props(props![
    prop!("text", PropType::Str).req().bind(Text).doc("The edited text. Two-way."),
    prop!("placeholder", PropType::Str),
    prop!("kind", TEXT_KIND).default("\"text\""),
    prop!("lines", PropType::Int),
    prop!("submitLabel", PropType::Str),
    prop!("autocapitalize", en!("none", "words", "sentences", "characters")),
    prop!("autocorrect", PropType::Bool),
    prop!("validate", PropType::Object(&VALIDATION)),
    prop!("sync", PropType::Str).default("\"debounce:300\"").doc(SYNC_DOC),
    prop!("mask", PropType::Str),
])
.events(events![
    EventSpec::new("submit").payload(&[PropSpec::new("value", PropType::Str).req()]),
    EventSpec::new("focusChange").payload(&[PropSpec::new("focused", PropType::Bool).req()]),
])
.slots(slots![SlotSpec::new("prefix").one(), SlotSpec::new("suffix").one()])
.interactive()
.a11y("textbox", TEXT_NAMED);

pub const SECURE_FIELD: ComponentSpec =
    ComponentSpec::new("SecureField", Group::Controls, Tier::One, "Password input with a reveal toggle.")
        .props(props![
            prop!("text", PropType::Str).req().bind(Text),
            prop!("placeholder", PropType::Str),
            prop!("sync", PropType::Str).default("\"submit\"").doc(SYNC_DOC),
        ])
        .events(events![EventSpec::new("submit").payload(&[PropSpec::new("value", PropType::Str).req()])])
        .interactive()
        .a11y("textbox", TEXT_NAMED);

pub const TEXT_EDITOR: ComponentSpec = ComponentSpec::new(
    "TextEditor",
    Group::Controls,
    Tier::One,
    "A multi-line editor with a two-way text binding. Agent writes merge instead of clobbering.",
)
.props(props![
    prop!("text", PropType::Str).req().bind(Text),
    prop!("placeholder", PropType::Str),
    prop!("font", en!("body", "mono")),
    prop!("readOnly", PropType::Bool),
    prop!("sync", PropType::Str).default("\"debounce:300\"").doc(SYNC_DOC),
    prop!("selection", PropType::Json).bind(Scalar).doc("Out-binding `{from, to}`."),
    prop!("spellcheck", PropType::Bool),
])
.events(events![EventSpec::new("focusChange").payload(&[PropSpec::new("focused", PropType::Bool).req()])])
.interactive()
.a11y("textbox", TEXT_NAMED);

pub const SEARCH_FIELD: ComponentSpec = ComponentSpec::new(
    "SearchField",
    Group::Controls,
    Tier::One,
    "A search input with scopes and suggestions.",
)
.props(props![
    prop!("text", PropType::Str).req().bind(Text),
    prop!("placeholder", PropType::Str),
    prop!("scopes", PropType::List(&PropType::Str)),
    prop!("suggestions", PropType::Source),
])
.events(events![
    EventSpec::new("submit").payload(&[PropSpec::new("value", PropType::Str).req()]),
    EventSpec::new("cancel"),
])
.interactive()
.a11y("searchbox", TEXT_NAMED);
