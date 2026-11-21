//! The runtime `ui` module that apps load with `require("vos:ui")` and the host embeds: `types/generated/ui.lua`.
//! Plain Lua (no annotations, no Luau syntax), so the contract tests can also run it with a native `lua`
//! (see [`module`]); the Luau declarations of the same constructors are [`crate::luau`].

mod ui;

pub use ui::{module, ACTION_BUILDERS};

use std::collections::BTreeMap;

use vos_domain::ui::{
    expr::Func,
    spec::{ComponentSpec, SlotSpec},
};

use crate::walk::split_group;

/// The Lua keywords, which cannot be bare table keys.
const RESERVED: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local", "nil",
    "not", "or", "repeat", "return", "then", "true", "until", "while",
];

/// A table key: bare when it is an identifier, `["..."]` otherwise.
pub(crate) fn key(name: &str) -> String {
    let ident = name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && !RESERVED.contains(&name);
    if ident {
        name.to_owned()
    } else {
        format!("[\"{name}\"]")
    }
}

/// The slot kinds of a component, as authors write them.
pub(crate) struct Slots<'a> {
    pub(crate) plain: Vec<&'a SlotSpec>,
    pub(crate) groups: BTreeMap<&'a str, Vec<&'a SlotSpec>>,
    pub(crate) prefixes: Vec<&'a SlotSpec>,
}

pub(crate) fn classify<'a>(slots: impl Iterator<Item = &'a SlotSpec>) -> Slots<'a> {
    let mut out = Slots { plain: Vec::new(), groups: BTreeMap::new(), prefixes: Vec::new() };
    for slot in slots {
        if slot.prefix {
            out.prefixes.push(slot);
        } else if let Some((group, _)) = split_group(slot.name) {
            out.groups.entry(group).or_default().push(slot);
        } else {
            out.plain.push(slot);
        }
    }
    out
}

pub(crate) fn group_name(prefix: &str) -> &str {
    prefix.trim_end_matches(':')
}

/// The Lua name of an expression function: `and`/`or`/`not`/`if` are keywords.
pub(crate) fn helper_name(f: Func) -> String {
    match f {
        Func::And => "all".to_owned(),
        Func::Or => "any".to_owned(),
        Func::Not => "negate".to_owned(),
        Func::If => "when".to_owned(),
        other => other.name().strip_prefix("fmt.").unwrap_or(other.name()).to_owned(),
    }
}

/// The one-line comment above a constructor.
pub(crate) fn describe(c: &ComponentSpec) -> String {
    format!("-- {} (Tier {}, since {})", c.doc, c.tier as u8, c.since)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_quoted_when_they_are_not_identifiers_or_are_reserved() {
        assert_eq!(key("title"), "title");
        assert_eq!(key("on_tap"), "on_tap");
        assert_eq!(key("type"), "type", "`type` is a function name, not a keyword, and fine as a field");
        assert_eq!(key("do"), "[\"do\"]");
        assert_eq!(key("end"), "[\"end\"]");
        assert_eq!(key("$ref"), "[\"$ref\"]");
        assert_eq!(key("2x"), "[\"2x\"]");
    }
}
