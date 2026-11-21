//! neoc's declarations of the `std:`, `lib:` and `vnd:` modules, as an app is offered them.
//!
//! `types/neoc.d.luau` (written by neoc-vm's `luau_decls` example) is one section per module, `-- ===== <module>
//! =====`, then a `require` section holding its `declare require` with one overload per module. An app's definitions
//! cannot load that file next to vos's: a second `declare require` replaces the first. So this takes the file apart:
//! the sections of the modules an app is offered are kept as they are, the overloads of those modules are handed to
//! [`defs`](super::defs) to be written into the one `declare require`, and the modules in
//! [`vos_domain::NOT_FOR_APPS`] are dropped (sections and overloads).
//!
//! This reads neoc's text; it does not depend on neoc-vm (`scripts/check-deps.ts` keeps vos-codegen to the domain).
//! A file that does not have the expected shape is an error, never a silently thinner definitions file.

/// What the app's definitions take from neoc's file.
#[derive(Debug, PartialEq, Eq)]
pub struct Offered {
    /// The sections of the offered modules, each from its marker line on, in neoc's order.
    pub sections: String,
    /// `(module, type)` of every offered module, in neoc's order: `("lib:json", "lib_json")`.
    pub overloads: Vec<(String, String)>,
    /// The modules that were left out.
    pub refused: Vec<String>,
}

const MARKER_START: &str = "-- ===== ";
const MARKER_END: &str = " =====";

fn marker(line: &str) -> Option<&str> {
    line.strip_prefix(MARKER_START)?.strip_suffix(MARKER_END)
}

/// `((path: "lib:json") -> lib_json)` -> `("lib:json", "lib_json")`.
pub(crate) fn overload(line: &str) -> Option<(String, String)> {
    let line = line.trim().trim_start_matches('&').trim();
    let rest = line.strip_prefix("((path: \"")?;
    let (module, rest) = rest.split_once("\") -> ")?;
    let ty = rest.strip_suffix(')')?;
    Some((module.to_owned(), ty.to_owned()))
}

/// Takes neoc's declarations apart, leaving out `not_for_apps`.
///
/// # Errors
/// The file has no `require` section, a line of it is not an overload, a module has a section but no overload (or
/// the other way round), or a module that is not offered to apps has no section: neoc's format changed, or the list
/// of refused modules names a module that is gone.
pub fn offered(neoc: &str, not_for_apps: &[&str]) -> Result<Offered, String> {
    let mut sections: Vec<(&str, String)> = Vec::new();
    for line in neoc.split_inclusive('\n') {
        if let Some(name) = marker(line.trim_end()) {
            sections.push((name, String::new()));
        }
        if let Some((_, body)) = sections.last_mut() {
            body.push_str(line);
        }
    }
    let Some(position) = sections.iter().position(|(name, _)| *name == "require") else {
        return Err("types/neoc.d.luau has no `-- ===== require =====` section".to_owned());
    };
    let (_, require) = sections.remove(position);
    if position != sections.len() {
        return Err("the `require` section of types/neoc.d.luau is not the last one".to_owned());
    }

    let mut all = Vec::new();
    for line in require.lines().skip(1).filter(|l| !l.trim().is_empty() && !l.starts_with("declare require"))
    {
        all.push(overload(line).ok_or_else(|| format!("not a `require` overload: {line:?}"))?);
    }
    for (module, _) in &all {
        if !sections.iter().any(|(name, _)| name == module) {
            return Err(format!("`require` has an overload for {module}, which has no section"));
        }
    }
    for (name, _) in &sections {
        if !all.iter().any(|(module, _)| module == name) {
            return Err(format!("{name} has a section but no `require` overload"));
        }
    }
    for module in not_for_apps {
        if !all.iter().any(|(m, _)| m == module) {
            return Err(format!("{module} is not offered to apps, but neoc has no such module"));
        }
    }

    let refused = |name: &str| not_for_apps.contains(&name);
    Ok(Offered {
        sections: sections.iter().filter(|(name, _)| !refused(name)).map(|(_, body)| body.as_str()).collect(),
        overloads: all.into_iter().filter(|(module, _)| !refused(module)).collect(),
        refused: not_for_apps.iter().map(|m| (*m).to_owned()).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "-- header\n\n-- ===== std:env =====\n\nexport type std_env = {}\n\n-- ===== lib:test =====\n\nexport type lib_test = {}\n\n-- ===== require =====\n\ndeclare require: \n\t((path: \"std:env\") -> std_env)\n\t& ((path: \"lib:test\") -> lib_test)\n";

    #[test]
    fn a_refused_module_loses_its_section_and_its_overload() {
        let got = offered(SAMPLE, &["lib:test"]).unwrap();
        assert!(got.sections.contains("export type std_env"), "{}", got.sections);
        assert!(!got.sections.contains("lib_test"), "{}", got.sections);
        assert_eq!(got.overloads, [("std:env".to_owned(), "std_env".to_owned())]);
        assert_eq!(got.refused, ["lib:test"]);
    }

    #[test]
    fn a_file_of_another_shape_is_an_error() {
        assert!(offered("-- no sections\n", &[]).unwrap_err().contains("no `-- ===== require =====`"));
        let no_overload = SAMPLE.replace("\t& ((path: \"lib:test\") -> lib_test)\n", "");
        assert!(offered(&no_overload, &[]).unwrap_err().contains("lib:test has a section but no"));
        let junk = format!("{SAMPLE}\tnot an overload\n");
        assert!(offered(&junk, &[]).unwrap_err().contains("not a `require` overload"));
        assert!(offered(SAMPLE, &["lib:gone"]).unwrap_err().contains("neoc has no such module"));
    }
}
