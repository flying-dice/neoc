//! Walks the component catalogue for the things every emitter needs.

use std::collections::BTreeMap;

use agentos_domain::ui::{
    components::{CATALOGUE, COMMON_EVENTS, COMMON_PROPS, FRAME, INSETS},
    spec::{EnumDef, EventSpec, ObjectDef, PropSpec, PropType},
};

/// Every named enum used by any prop, event payload or object field, by name.
pub fn named_enums() -> BTreeMap<&'static str, &'static EnumDef> {
    let mut found = BTreeMap::new();
    for_each_prop(&mut |p| visit_type(&p.ty, &mut found, &mut BTreeMap::new()));
    found
}

/// Every named object type used by any prop, event payload or object field, by name.
pub fn named_objects() -> BTreeMap<&'static str, &'static ObjectDef> {
    let mut found = BTreeMap::new();
    // `padding` and `frame` are their own prop types, backed by these two objects
    found.insert(INSETS.name, &INSETS);
    found.insert(FRAME.name, &FRAME);
    for_each_prop(&mut |p| visit_type(&p.ty, &mut BTreeMap::new(), &mut found));
    found
}

fn for_each_prop(visit: &mut dyn FnMut(&'static PropSpec)) {
    for p in COMMON_PROPS {
        visit(p);
    }
    for e in COMMON_EVENTS {
        e.payload.iter().for_each(&mut *visit);
    }
    for c in CATALOGUE {
        c.props.iter().for_each(&mut *visit);
        for e in c.events {
            e.payload.iter().for_each(&mut *visit);
        }
    }
}

fn visit_type(
    ty: &PropType,
    enums: &mut BTreeMap<&'static str, &'static EnumDef>,
    objects: &mut BTreeMap<&'static str, &'static ObjectDef>,
) {
    match *ty {
        PropType::Enum(def) if !def.name.is_empty() => {
            enums.insert(def.name, def);
        }
        PropType::List(inner) => visit_type(inner, enums, objects),
        PropType::Union(options) => options.iter().for_each(|o| visit_type(o, enums, objects)),
        PropType::Object(def) => {
            if !def.name.is_empty() {
                objects.insert(def.name, def);
            }
            for f in def.fields {
                visit_type(&f.ty, enums, objects);
            }
        }
        _ => {}
    }
}

/// Does the type accept a source handle?
pub fn contains_source(ty: &PropType) -> bool {
    match ty {
        PropType::Source => true,
        PropType::Union(options) => options.iter().any(contains_source),
        PropType::List(inner) => contains_source(inner),
        _ => false,
    }
}

/// The props that apply to a component, as a `Vec` (own, then the common ones it does not override).
pub fn effective_props(c: &agentos_domain::ui::spec::ComponentSpec) -> Vec<&'static PropSpec> {
    c.effective_props().collect()
}

/// The events that apply to a component.
pub fn effective_events(c: &agentos_domain::ui::spec::ComponentSpec) -> Vec<&'static EventSpec> {
    c.effective_events().collect()
}

/// `toolbar.leading` -> `("toolbar", "leading")`.
pub fn split_group(slot: &str) -> Option<(&str, &str)> {
    slot.split_once('.')
}
