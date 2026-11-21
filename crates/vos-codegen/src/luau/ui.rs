//! The `ui` module in Luau declarations: every component's props type, the `vos_ui` table type that
//! `require("vos:ui")` returns, `ui.fmt` and `ui.act`.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use vos_domain::ui::{
    components::{CATALOGUE, STACK_SUGAR},
    expr::{Arity, Func},
    spec::{ComponentSpec, SlotScope},
};

use super::{field_type, key, named, optional, payload_type, value_type};
use crate::lua::{classify, group_name, helper_name};
use crate::walk::{effective_events, effective_props, split_group};

/// One field of a props type: its type and whether the author must give it.
struct Field {
    ty: String,
    required: bool,
    doc: &'static str,
}

/// The fields of a props type, in the order they were first put.
#[derive(Default)]
struct Fields {
    by_name: BTreeMap<String, Field>,
    order: Vec<String>,
}

impl Fields {
    fn put(&mut self, name: String, field: Field) {
        if self.by_name.insert(name.clone(), field).is_none() {
            self.order.push(name);
        }
    }
}

/// The one-line doc above a component's props type and constructor.
fn describe(c: &ComponentSpec) -> String {
    format!("{} (Tier {}, since {})", c.doc, c.tier as u8, c.since)
}

fn props_type(c: &ComponentSpec) -> String {
    let child = named("Child");
    let mut fields = Fields::default();

    fields.put("key".to_owned(), Field { ty: "string".to_owned(), required: c.interactive, doc: "" });
    for p in effective_props(c) {
        // a positional prop is given as the first entry, so it is never required by name
        let required = p.required && c.positional != Some(p.name);
        fields.put(key(p.name), Field { ty: field_type(p), required, doc: p.doc });
    }
    for e in effective_events(c) {
        fields.put(
            e.prop_name(),
            Field {
                ty: format!("{}<{}>", named("Handler"), payload_type(e.payload)),
                required: false,
                doc: "",
            },
        );
    }
    // slots: a slot that shares a name with a prop widens that prop's type (a string, or a node)
    let slots = classify(c.effective_slots());
    for slot in &slots.plain {
        let row = slot.scope == Some(SlotScope::Row);
        let slot_ty = if row {
            format!("{child} | (({}) -> {})", named("RowRefs"), named("Node"))
        } else {
            child.clone()
        };
        let name = key(slot.name);
        match fields.by_name.get_mut(&name) {
            Some(existing) => {
                existing.ty = format!("{} | {slot_ty}", existing.ty);
                existing.required = false;
            }
            None => fields.put(name, Field { ty: slot_ty, required: slot.required, doc: "" }),
        }
    }
    for (group, members) in &slots.groups {
        let inner: Vec<String> = members
            .iter()
            .map(|m| {
                let (_, sub) = split_group(m.name).unwrap_or(("", m.name));
                format!("{}: {}", key(sub), optional(&child))
            })
            .collect();
        fields.put(key(group), Field { ty: format!("{{ {} }}", inner.join(", ")), required: false, doc: "" });
    }
    for prefix in &slots.prefixes {
        fields.put(
            key(group_name(prefix.name)),
            Field { ty: format!("{{ [string]: {child} }}"), required: false, doc: "" },
        );
    }

    let mut out = String::new();
    let _ = writeln!(out, "--- {}", c.doc);
    let _ = writeln!(out, "export type {} = {{", named(&format!("{}Props", c.name)));
    for name in &fields.order {
        let field = &fields.by_name[name];
        let ty = if field.required { field.ty.clone() } else { optional(&field.ty) };
        let doc = field.doc.replace('\n', " ");
        if !doc.is_empty() {
            let _ = writeln!(out, "\t--- {doc}");
        }
        let _ = writeln!(out, "\t{name}: {ty},");
    }
    // positional entries
    let positional = c.positional.and_then(|p| c.prop(p));
    let expr = named("Expr");
    let entry = match (positional, c.children) {
        (Some(p), vos_domain::ui::spec::Children::None) => Some(format!("{expr}<{}>", value_type(&p.ty))),
        (Some(p), _) => Some(format!("{expr}<{}> | {child}", value_type(&p.ty))),
        (None, vos_domain::ui::spec::Children::None) => None,
        (None, _) => Some(child.clone()),
    };
    if let Some(entry) = entry {
        let _ = writeln!(out, "\t[number]: {entry},");
    }
    out.push_str("}\n\n");
    out
}

/// Parameters of a function of `arity`: `a: any, b: any?` / `first: any, ...any`.
fn params(arity: Arity) -> String {
    let names = ["a", "b", "c"];
    match arity.max {
        None => "first: any, ...any".to_owned(),
        Some(max) => names
            .iter()
            .enumerate()
            .take(max)
            .map(|(i, name)| format!("{name}: {}", if i < arity.min { "any" } else { "any?" }))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// The `vos_ui` table type, and the two types it nests (`ui.fmt`, `ui.act`).
fn module_type() -> String {
    let (node, fnty) = (named("Node"), named("Fn"));
    let mut out = String::new();
    let _ = writeln!(out, "export type {} = {{", named("UiFormat"));
    for f in Func::ALL.iter().filter(|f| f.name().starts_with("fmt.")) {
        let _ = writeln!(out, "\t--- {}\n\t{}: ({}) -> {fnty},", f.doc(), helper_name(*f), params(f.arity()));
    }
    out.push_str("}\n\n");
    out.push_str(ACTIONS);
    let _ = writeln!(out, "export type {} = {{", named("ui"));
    for c in CATALOGUE {
        let _ = writeln!(
            out,
            "\t--- {}\n\t{}: (props: {}) -> {node},",
            describe(c),
            c.name,
            named(&format!("{}Props", c.name))
        );
    }
    for (sugar, axis) in STACK_SUGAR {
        let which = if *axis == "v" { "vertical" } else { "horizontal" };
        let _ =
            writeln!(out, "\t--- A {which} `Stack`.\n\t{sugar}: (props: {}) -> {node},", named("StackProps"));
    }
    let _ = writeln!(
        out,
        "\t--- Maps a Lua list to children. `nil` results are skipped.\n\
\teach: <T>(list: {{ T }}, body: (item: T, index: number) -> {node}?) -> {{ {node} }},"
    );
    for f in Func::ALL.iter().filter(|f| !f.name().starts_with("fmt.")) {
        let _ = writeln!(out, "\t--- {}\n\t{}: ({}) -> {fnty},", f.doc(), helper_name(*f), params(f.arity()));
    }
    let _ = writeln!(out, "\tfmt: {},\n\tact: {},", named("UiFormat"), named("UiActions"));
    out.push_str("}\n\n");
    out
}

/// Everything the `ui` module adds to the definitions: the props types, then the module type.
pub fn declarations() -> String {
    let mut out = String::from("-- ===== vos:ui =====\n-- `local ui = require(\"vos:ui\")` in app code.\n\n");
    for c in CATALOGUE {
        out.push_str(&props_type(c));
    }
    out.push_str(&module_type());
    out
}

const ACTIONS: &str = r#"--- Client actions: run on the client with no round trip. Handlers take one, or a list mixing actions and functions.
export type vos_UiActions = {
	--- Write a client-managed or app-state ref.
	set: (target: vos_Ref<any> | string, value: any) -> vos_Action,
	--- Flip a boolean ref.
	toggle: (target: vos_Ref<any> | string) -> vos_Action,
	--- Push a route (`{ name = "note", id = 7 }`) onto a navigation stack (the optional `stack` names it).
	push: (route: { name: string, [string]: any }, stack: string?) -> vos_Action,
	--- Pop the top route.
	pop: () -> vos_Action,
	--- Pop to the root screen.
	popToRoot: () -> vos_Action,
	--- Move focus to the node with this key.
	focus: (key: string) -> vos_Action,
	--- Scroll to the node with this key.
	scrollTo: (key: string) -> vos_Action,
	--- Copy text to the clipboard.
	copy: (text: vos_Expr<string>) -> vos_Action,
	--- Report an event to the app (the handler of the node still runs on the host).
	emit: (event: string?, payload: { [string]: any }?) -> vos_Action,
	--- Run actions in order.
	seq: (actions: { vos_Action }) -> vos_Action,
}

"#;
