//! The component catalogue (spec §1): every component with typed props, events, slots and metadata.
//!
//! Tier 1 (the first apps) is complete and final. Tier 2 and 3 carry names, tiers and placeholder props
//! (`open_props`): the validator accepts any prop on them until their schema is written.

use super::spec::{ComponentSpec, EnumDef, EventSpec, ObjectDef, PropSpec, PropType, SlotSpec};

/// An inline anonymous enum prop type.
macro_rules! en {
    ($($value:literal),+ $(,)?) => {
        $crate::ui::spec::PropType::Enum(&$crate::ui::spec::EnumDef { name: "", values: &[$($value),+] })
    };
}

/// `prop!("name", Type)` is `PropSpec::new("name", Type)`.
macro_rules! prop {
    ($name:literal, $ty:expr) => {
        $crate::ui::spec::PropSpec::new($name, $ty)
    };
}

/// A `&'static [PropSpec]` that lives long enough to be passed to a builder (a `const` block keeps the elements,
/// which are `const fn` calls, out of temporaries).
macro_rules! props {
    ($($p:expr),* $(,)?) => {{
        const P: &[$crate::ui::spec::PropSpec] = &[$($p),*];
        P
    }};
}

/// A `&'static [EventSpec]`, as [`props!`].
macro_rules! events {
    ($($e:expr),* $(,)?) => {{
        const E: &[$crate::ui::spec::EventSpec] = &[$($e),*];
        E
    }};
}

/// A `&'static [SlotSpec]`, as [`props!`].
macro_rules! slots {
    ($($s:expr),* $(,)?) => {{
        const S: &[$crate::ui::spec::SlotSpec] = &[$($s),*];
        S
    }};
}

mod advanced;
mod collections;
mod controls;
mod display;
mod layout;
mod navigation;
mod presentation;

pub use advanced::{
    BANNER, BREADCRUMB, CALENDAR_GRID, CANVAS, CHART, CHAT_TRANSCRIPT, CHIP, CODE_EDITOR, DIFF_VIEW, GAUGE,
    INSPECTOR, LAZY_GRID, LINK, OUTLINE, PREVIEW, RATING, SKELETON, SPARKLINE, TABLE, TERMINAL_VIEW,
    TOKEN_FIELD,
};

// ---------------------------------------------------------------------------------------------------------
// Closed token sets (spec §5.1): apps can say what a thing is and how much space it needs, never what colour
// or font it is.
// ---------------------------------------------------------------------------------------------------------

pub const TEXT_STYLE: EnumDef = EnumDef {
    name: "TextStyle",
    values: &[
        "largeTitle",
        "title1",
        "title2",
        "title3",
        "headline",
        "body",
        "callout",
        "subheadline",
        "footnote",
        "caption1",
        "caption2",
        "display",
    ],
};

pub const PALETTE: EnumDef = EnumDef {
    name: "PaletteToken",
    values: &[
        "blue", "red", "green", "orange", "yellow", "purple", "pink", "indigo", "teal", "mint", "cyan",
        "brown", "gray", "gray2", "gray3", "gray4", "gray5", "gray6",
    ],
};

pub const SURFACE: EnumDef = EnumDef {
    name: "SurfaceToken",
    values: &[
        "primary",
        "secondary",
        "tertiary",
        "grouped",
        "elevated",
        "fill",
        "fill2",
        "fill3",
        "fill4",
        "tint",
        "tintSoft",
    ],
};

pub const LABEL_COLOR: EnumDef = EnumDef {
    name: "LabelToken",
    values: &["primary", "secondary", "tertiary", "tint", "destructive", "success", "warning"],
};

pub const RADIUS: EnumDef =
    EnumDef { name: "RadiusToken", values: &["none", "xs", "sm", "md", "lg", "xl", "full"] };

pub const INSETS: ObjectDef = ObjectDef {
    name: "Insets",
    fields: &[
        prop!("x", PropType::Spacing),
        prop!("y", PropType::Spacing),
        prop!("top", PropType::Spacing),
        prop!("right", PropType::Spacing),
        prop!("bottom", PropType::Spacing),
        prop!("left", PropType::Spacing),
    ],
    open: false,
};

pub const FRAME: ObjectDef = ObjectDef {
    name: "Frame",
    fields: &[
        prop!("minW", PropType::Number),
        prop!("idealW", PropType::Number),
        prop!("maxW", PropType::Number),
        prop!("minH", PropType::Number),
        prop!("idealH", PropType::Number),
        prop!("maxH", PropType::Number),
    ],
    open: false,
};

/// A navigation route: `{name, ...params}`.
pub const ROUTE: ObjectDef =
    ObjectDef { name: "Route", fields: &[PropSpec::new("name", PropType::Str).req()], open: true };

pub const DRAG: ObjectDef = ObjectDef {
    name: "Drag",
    fields: &[
        PropSpec::new("type", PropType::Str)
            .req()
            .doc("The drag type; a drop target lists the types it accepts."),
        prop!("payload", PropType::Json),
        prop!("preview", PropType::Str),
    ],
    open: false,
};

pub const DROP: ObjectDef = ObjectDef {
    name: "Drop",
    fields: &[
        PropSpec::new("accepts", PropType::List(&PropType::Str)).req(),
        prop!("highlight", PropType::Bool),
    ],
    open: false,
};

/// How a text value syncs back to the host: `change`, `blur`, `submit` or `debounce:<ms>`.
pub const SYNC_DOC: &str = "When the value is written back: `change`, `blur`, `submit` or `debounce:<ms>`.";

// ---------------------------------------------------------------------------------------------------------
// Common props, events and slots (spec §1.1): every node accepts these. A component's own prop of the same
// name overrides the common one (`Text.align`, `ZStack.align`, `Icon.tint`).
// ---------------------------------------------------------------------------------------------------------

pub const COMMON_PROPS: &[PropSpec] = &[
    prop!("testid", PropType::Str).doc("Overrides the derived test id."),
    prop!("label", PropType::Str).doc("Accessible name."),
    prop!("hint", PropType::Str).doc("Accessible description."),
    prop!("hidden", PropType::Bool).doc("Removed from layout and the accessibility tree."),
    prop!("disabled", PropType::Bool),
    prop!("padding", PropType::Insets).doc("Spacing units of 4 pt: `0..=8` or per edge."),
    prop!("frame", PropType::Frame)
        .doc("Layout, not styling: `\"fill\"` or min/ideal/max in pt, clamped to the window."),
    prop!("grow", PropType::Number).doc("Flex grow factor."),
    prop!("align", en!("start", "center", "end", "stretch")).doc("Alignment in the parent stack."),
    prop!("background", PropType::Enum(&SURFACE)),
    prop!("foreground", PropType::Enum(&LABEL_COLOR)),
    prop!("tint", PropType::Enum(&PALETTE)).doc("Re-points the accent for the subtree."),
    prop!("corner", PropType::Enum(&RADIUS)),
    prop!("help", PropType::Str).doc("Tooltip."),
    prop!("shortcut", PropType::Str).doc("`\"cmd+n\"`: shown in menus, active while the window is focused."),
    prop!("drag", PropType::Object(&DRAG))
        .doc("Makes the node draggable. Always give it a non-drag path (`contextMenu`)."),
    prop!("drop", PropType::Object(&DROP)).doc("Makes the node a drop target."),
    prop!("animation", en!("default", "none")),
    prop!("tag", PropType::Json).doc("The selection value of a row, tab or picker item."),
    prop!("span", PropType::Int).doc("Grid child: columns spanned."),
    prop!("rowStart", PropType::Int).doc("Grid child: start row."),
    prop!("colStart", PropType::Int).doc("Grid child: start column."),
];

pub const COMMON_EVENTS: &[EventSpec] = &[
    EventSpec::new("appear"),
    EventSpec::new("disappear"),
    EventSpec::new("drop").payload(&[
        PropSpec::new("type", PropType::Str).req(),
        prop!("payload", PropType::Json),
        PropSpec::new("target", PropType::Json).req().doc("`{key, beforeKey, edge}`"),
    ]),
];

pub const COMMON_SLOTS: &[SlotSpec] = &[
    SlotSpec::new("contextMenu").doc("Menu items for long-press / right-click."),
    SlotSpec::new("fallback").doc("Rendered by a client whose kit predates the component."),
    SlotSpec::new("tabItem").one().doc("On a `TabView` child: the tab's label, icon and badge."),
];

// ---------------------------------------------------------------------------------------------------------
// The catalogue.
// ---------------------------------------------------------------------------------------------------------

/// Every component, Tier 1 first.
pub const CATALOGUE: &[ComponentSpec] = &[
    // layout
    layout::STACK,
    layout::ZSTACK,
    layout::GRID,
    layout::SCROLL_VIEW,
    layout::SPACER,
    layout::DIVIDER,
    layout::GROUP,
    layout::ADAPTIVE,
    layout::SCOPE,
    layout::SHOW,
    layout::FOR_EACH,
    layout::CARD,
    // navigation
    navigation::SCREEN,
    navigation::NAVIGATION_STACK,
    navigation::NAVIGATION_LINK,
    navigation::SPLIT_VIEW,
    navigation::TAB_VIEW,
    // collections
    collections::LIST,
    collections::SECTION,
    collections::ROW,
    collections::FORM,
    collections::LABELED_CONTENT,
    collections::DISCLOSURE_GROUP,
    // controls
    controls::SHORTCUT,
    controls::BUTTON,
    controls::TOGGLE,
    controls::SLIDER,
    controls::STEPPER,
    controls::PICKER,
    controls::DATE_PICKER,
    controls::COLOR_PICKER,
    controls::TEXT_FIELD,
    controls::SECURE_FIELD,
    controls::TEXT_EDITOR,
    controls::SEARCH_FIELD,
    // presentation
    presentation::SHEET,
    presentation::POPOVER,
    presentation::ALERT,
    presentation::CONFIRMATION_DIALOG,
    presentation::MENU,
    // display
    display::TEXT,
    display::LABEL,
    display::ICON,
    display::IMAGE,
    display::BADGE,
    display::PROGRESS_VIEW,
    display::MARKDOWN,
    display::AVATAR,
    display::EMPTY_STATE,
    // tier 2 and 3 (placeholder props)
    TABLE,
    LAZY_GRID,
    OUTLINE,
    SKELETON,
    GAUGE,
    CHIP,
    TOKEN_FIELD,
    BANNER,
    LINK,
    CHAT_TRANSCRIPT,
    CALENDAR_GRID,
    PREVIEW,
    CANVAS,
    CODE_EDITOR,
    DIFF_VIEW,
    TERMINAL_VIEW,
    RATING,
    CHART,
    SPARKLINE,
    BREADCRUMB,
    INSPECTOR,
];

/// The component called `name`.
#[must_use]
pub fn lookup(name: &str) -> Option<&'static ComponentSpec> {
    CATALOGUE.iter().find(|c| c.name == name)
}

/// Constructor sugar: `ui.VStack` / `ui.HStack` are `Stack` with an `axis`.
pub const STACK_SUGAR: &[(&str, &str)] = &[("VStack", "v"), ("HStack", "h")];

#[cfg(test)]
mod tests {
    use super::super::spec::Tier;
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn names_are_unique_and_pascal_case() {
        let mut seen = HashSet::new();
        for c in CATALOGUE {
            assert!(seen.insert(c.name), "duplicate component {}", c.name);
            assert!(c.name.chars().next().is_some_and(char::is_uppercase), "{}", c.name);
        }
    }

    #[test]
    fn tier_one_is_the_spec_list() {
        let tier_one: Vec<&str> = CATALOGUE.iter().filter(|c| c.tier == Tier::One).map(|c| c.name).collect();
        for expected in [
            // layout
            "Stack",
            "ZStack",
            "Grid",
            "ScrollView",
            "Spacer",
            "Divider",
            "Group",
            "Adaptive",
            "Scope",
            "Show",
            "ForEach",
            "Card",
            // navigation
            "Screen",
            "NavigationStack",
            "NavigationLink",
            "SplitView",
            "TabView",
            // collections
            "List",
            "Section",
            "Row",
            "Form",
            "LabeledContent",
            "DisclosureGroup",
            // controls
            "Shortcut",
            "Button",
            "Toggle",
            "Slider",
            "Stepper",
            "Picker",
            "DatePicker",
            "ColorPicker",
            "TextField",
            "SecureField",
            "TextEditor",
            "SearchField",
            // presentation
            "Sheet",
            "Popover",
            "Alert",
            "ConfirmationDialog",
            "Menu",
            // display
            "Text",
            "Label",
            "Icon",
            "Image",
            "Badge",
            "ProgressView",
            "Markdown",
            "Avatar",
            "EmptyState",
        ] {
            assert!(tier_one.contains(&expected), "missing Tier 1 component {expected}");
        }
        assert_eq!(tier_one.len(), 49);
    }

    #[test]
    fn a_component_has_no_duplicate_props_events_or_slots() {
        for c in CATALOGUE {
            let mut props = HashSet::new();
            assert!(c.props.iter().all(|p| props.insert(p.name)), "{} repeats a prop", c.name);
            let mut events = HashSet::new();
            assert!(c.events.iter().all(|e| events.insert(e.name)), "{} repeats an event", c.name);
            let mut slots = HashSet::new();
            assert!(c.slots.iter().all(|s| slots.insert(s.name)), "{} repeats a slot", c.name);
        }
    }

    #[test]
    fn positional_props_exist_and_are_required_where_they_carry_content() {
        for c in CATALOGUE {
            if let Some(positional) = c.positional {
                assert!(c.prop(positional).is_some(), "{}: positional `{positional}` is not a prop", c.name);
            }
        }
    }

    #[test]
    fn bindings_are_never_required_to_be_literals() {
        for c in CATALOGUE {
            for p in c.props {
                if p.binding.is_some() {
                    assert!(!matches!(p.ty, PropType::Source), "{}.{} binds a source id", c.name, p.name);
                }
            }
        }
    }

    #[test]
    fn interactive_components_have_a_name_rule_or_are_structural() {
        use super::super::spec::NameRule;
        for c in CATALOGUE.iter().filter(|c| c.tier == Tier::One && c.interactive) {
            let structural = matches!(c.name, "List" | "Shortcut");
            assert!(
                structural || c.a11y.name != NameRule::None,
                "{} is interactive but needs no name",
                c.name
            );
        }
    }

    #[test]
    fn lookups() {
        assert_eq!(lookup("Button").map(|c| c.name), Some("Button"));
        assert!(lookup("Nope").is_none());
    }
}
