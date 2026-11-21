//! The component schema as Rust data: typed props, events, slots and the metadata that no plain type system
//! carries (tier, `since`, accessibility rules, the positional argument, interactivity).
//!
//! These tables are the single source for the validator ([`super::validate`]), the TypeScript types, the
//! Lua definitions and constructors, and the reference docs (`vos-codegen`). They are `const` data, so
//! the catalogue costs nothing at run time and cannot be mutated.

use super::version::KitVersion;

/// A closed set of string values, optionally named (a named set is emitted as a type alias).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnumDef {
    /// `""` for an inline anonymous set.
    pub name: &'static str,
    pub values: &'static [&'static str],
}

/// A named or anonymous object type with typed fields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ObjectDef {
    /// `""` for an inline anonymous object.
    pub name: &'static str,
    pub fields: &'static [PropSpec],
    /// Extra fields are allowed (a route `{name, ...params}`).
    pub open: bool,
}

/// What a prop holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PropType {
    Str,
    Number,
    Int,
    Bool,
    /// Any JSON value.
    Json,
    Enum(&'static EnumDef),
    /// Spacing in 4 pt units: an integer `0..=8`.
    Spacing,
    /// `padding`: a spacing, or `{x, y, top, right, bottom, left}`.
    Insets,
    /// `frame`: `"fill"` or `{minW, idealW, maxW, minH, idealH, maxH}` in pt.
    Frame,
    /// The id of a source declared for the view (`Source<R>`).
    Source,
    List(&'static PropType),
    Object(&'static ObjectDef),
    Union(&'static [PropType]),
}

/// How the host merges a two-way write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingKind {
    /// Three-way merge; overlapping edits go to the human.
    Text,
    /// A control value: the human wins.
    Scalar,
    /// A set of values: union or difference of the user's delta.
    Collection,
}

impl BindingKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Scalar => "scalar",
            Self::Collection => "collection",
        }
    }
}

/// One prop (or one field of an event payload or an object type).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PropSpec {
    pub name: &'static str,
    pub ty: PropType,
    pub required: bool,
    /// `Some`: a `Binding<T>`: the value must be a writable ref, and the control also writes it.
    pub binding: Option<BindingKind>,
    pub default: Option<&'static str>,
    pub doc: &'static str,
}

impl PropSpec {
    #[must_use]
    pub const fn new(name: &'static str, ty: PropType) -> Self {
        Self { name, ty, required: false, binding: None, default: None, doc: "" }
    }
    #[must_use]
    pub const fn req(self) -> Self {
        Self { required: true, ..self }
    }
    #[must_use]
    pub const fn bind(self, kind: BindingKind) -> Self {
        Self { binding: Some(kind), ..self }
    }
    #[must_use]
    pub const fn default(self, value: &'static str) -> Self {
        Self { default: Some(value), ..self }
    }
    #[must_use]
    pub const fn doc(self, text: &'static str) -> Self {
        Self { doc: text, ..self }
    }
}

/// What a `{$ref}` inside a slot may point at, beyond the global namespaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotScope {
    /// A row template: `row.*` is in scope.
    Row,
    /// A navigation destination: `route.*` is in scope.
    Route,
}

/// A named child list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotSpec {
    /// The slot name, or its prefix when `prefix` (`dest:` matches `dest:note`).
    pub name: &'static str,
    pub prefix: bool,
    /// At most this many nodes; `None` = any number.
    pub max: Option<u8>,
    pub scope: Option<SlotScope>,
    pub required: bool,
    pub doc: &'static str,
}

impl SlotSpec {
    #[must_use]
    pub const fn new(name: &'static str) -> Self {
        Self { name, prefix: false, max: None, scope: None, required: false, doc: "" }
    }
    #[must_use]
    pub const fn one(self) -> Self {
        Self { max: Some(1), ..self }
    }
    #[must_use]
    pub const fn prefix(self) -> Self {
        Self { prefix: true, ..self }
    }
    #[must_use]
    pub const fn scope(self, scope: SlotScope) -> Self {
        Self { scope: Some(scope), ..self }
    }
    #[must_use]
    pub const fn req(self) -> Self {
        Self { required: true, ..self }
    }
    #[must_use]
    pub const fn doc(self, text: &'static str) -> Self {
        Self { doc: text, ..self }
    }

    /// Does this slot spec describe a slot called `name`?
    #[must_use]
    pub fn matches(&self, name: &str) -> bool {
        if self.prefix {
            name.strip_prefix(self.name).is_some_and(|rest| !rest.is_empty())
        } else {
            self.name == name
        }
    }
}

/// An event a component reports. The wire name is `tap`; the Lua prop is `onTap`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EventSpec {
    pub name: &'static str,
    pub payload: &'static [PropSpec],
    pub doc: &'static str,
}

impl EventSpec {
    #[must_use]
    pub const fn new(name: &'static str) -> Self {
        Self { name, payload: &[], doc: "" }
    }
    #[must_use]
    pub const fn payload(self, fields: &'static [PropSpec]) -> Self {
        Self { payload: fields, ..self }
    }
    #[must_use]
    pub const fn doc(self, text: &'static str) -> Self {
        Self { doc: text, ..self }
    }

    /// The Lua handler prop (`onTap`).
    #[must_use]
    pub fn prop_name(&self) -> String {
        handler_prop(self.name)
    }
}

/// `tap` -> `onTap`.
#[must_use]
pub fn handler_prop(event: &str) -> String {
    let mut chars = event.chars();
    chars.next().map_or_else(
        || "on".to_owned(),
        |first| format!("on{}{}", first.to_ascii_uppercase(), chars.as_str()),
    )
}

/// What the default slot accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Children {
    None,
    Any,
    /// At most this many.
    Max(u8),
}

/// How an accessible name is established.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameRule {
    /// No name required (decorative or structural).
    None,
    /// At least one of these props must be set (non-empty), or a child / slot supplies the content.
    OneOf { props: &'static [&'static str], children: bool },
    /// An `alt` text, or `decorative = true`.
    AltOrDecorative,
}

/// The accessibility contract of a component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct A11y {
    /// The ARIA role the kit gives it (`""` = none).
    pub role: &'static str,
    pub name: NameRule,
}

/// The rollout tier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    /// The first apps; complete before the first app ships.
    One = 1,
    Two = 2,
    Three = 3,
}

/// The catalogue group, used to order the docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    Layout,
    Navigation,
    Collections,
    Controls,
    Presentation,
    Display,
    Advanced,
}

impl Group {
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Layout => "Layout",
            Self::Navigation => "Navigation",
            Self::Collections => "Collections",
            Self::Controls => "Controls",
            Self::Presentation => "Presentation",
            Self::Display => "Display",
            Self::Advanced => "Advanced",
        }
    }
}

/// One catalogue entry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComponentSpec {
    pub name: &'static str,
    pub group: Group,
    pub tier: Tier,
    /// The kit version that introduced the component.
    pub since: KitVersion,
    pub deprecated: Option<&'static str>,
    /// Takes input: needs an explicit key and a name.
    pub interactive: bool,
    pub a11y: A11y,
    /// `ui.Text { "hi" }` sets this prop.
    pub positional: Option<&'static str>,
    pub props: &'static [PropSpec],
    pub events: &'static [EventSpec],
    pub slots: &'static [SlotSpec],
    pub children: Children,
    /// Props are not final: any prop is accepted (Tier 2 and 3 placeholders).
    pub open_props: bool,
    pub doc: &'static str,
}

impl ComponentSpec {
    #[must_use]
    pub const fn new(name: &'static str, group: Group, tier: Tier, doc: &'static str) -> Self {
        Self {
            name,
            group,
            tier,
            since: KitVersion::new(1, 0),
            deprecated: None,
            interactive: false,
            a11y: A11y { role: "", name: NameRule::None },
            positional: None,
            props: &[],
            events: &[],
            slots: &[],
            children: Children::None,
            open_props: false,
            doc,
        }
    }
    #[must_use]
    pub const fn props(self, props: &'static [PropSpec]) -> Self {
        Self { props, ..self }
    }
    #[must_use]
    pub const fn events(self, events: &'static [EventSpec]) -> Self {
        Self { events, ..self }
    }
    #[must_use]
    pub const fn slots(self, slots: &'static [SlotSpec]) -> Self {
        Self { slots, ..self }
    }
    #[must_use]
    pub const fn children(self, children: Children) -> Self {
        Self { children, ..self }
    }
    #[must_use]
    pub const fn interactive(self) -> Self {
        Self { interactive: true, ..self }
    }
    #[must_use]
    pub const fn positional(self, prop: &'static str) -> Self {
        Self { positional: Some(prop), ..self }
    }
    #[must_use]
    pub const fn a11y(self, role: &'static str, name: NameRule) -> Self {
        Self { a11y: A11y { role, name }, ..self }
    }
    #[must_use]
    pub const fn open(self) -> Self {
        Self { open_props: true, ..self }
    }
    #[must_use]
    pub const fn since(self, major: u16, minor: u16) -> Self {
        Self { since: KitVersion::new(major, minor), ..self }
    }

    /// The prop `name` of this component: its own, else a common prop.
    #[must_use]
    pub fn prop(&self, name: &str) -> Option<&'static PropSpec> {
        self.props.iter().chain(super::components::COMMON_PROPS).find(|p| p.name == name)
    }

    /// The event `name` of this component: its own, else a common event.
    #[must_use]
    pub fn event(&self, name: &str) -> Option<&'static EventSpec> {
        self.events.iter().chain(super::components::COMMON_EVENTS).find(|e| e.name == name)
    }

    /// The slot spec for a slot called `name`.
    #[must_use]
    pub fn slot(&self, name: &str) -> Option<&'static SlotSpec> {
        self.slots.iter().chain(super::components::COMMON_SLOTS).find(|s| s.matches(name))
    }

    /// Every prop that applies: the component's own, then the common props it does not override.
    pub fn effective_props(&self) -> impl Iterator<Item = &'static PropSpec> + '_ {
        let own = self.props;
        own.iter().chain(
            super::components::COMMON_PROPS.iter().filter(move |c| !own.iter().any(|p| p.name == c.name)),
        )
    }

    /// Every event that applies: own, then common.
    pub fn effective_events(&self) -> impl Iterator<Item = &'static EventSpec> + '_ {
        let own = self.events;
        own.iter().chain(
            super::components::COMMON_EVENTS.iter().filter(move |c| !own.iter().any(|e| e.name == c.name)),
        )
    }

    /// Every slot that applies: own, then common.
    pub fn effective_slots(&self) -> impl Iterator<Item = &'static SlotSpec> + '_ {
        let own = self.slots;
        own.iter().chain(
            super::components::COMMON_SLOTS.iter().filter(move |c| !own.iter().any(|s| s.name == c.name)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handler_props_are_derived_from_event_names() {
        assert_eq!(handler_prop("tap"), "onTap");
        assert_eq!(handler_prop("searchSubmit"), "onSearchSubmit");
        assert_eq!(EventSpec::new("focusChange").prop_name(), "onFocusChange");
    }

    #[test]
    fn slot_matching_supports_prefixes() {
        let dest = SlotSpec::new("dest:").prefix();
        assert!(dest.matches("dest:note"));
        assert!(!dest.matches("dest:"));
        assert!(!dest.matches("row"));
        assert!(SlotSpec::new("row").matches("row"));
    }

    #[test]
    fn builders_compose_in_const_context() {
        const BUTTON: ComponentSpec = ComponentSpec::new("X", Group::Controls, Tier::One, "doc")
            .interactive()
            .positional("title")
            .since(1, 2);
        let (interactive, positional, since) = (BUTTON.interactive, BUTTON.positional, BUTTON.since);
        assert!(interactive);
        assert_eq!(positional, Some("title"));
        assert_eq!(since, KitVersion::new(1, 2));
    }
}
