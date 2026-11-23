//! neoc's declarations of the `std:`, `lib:` and `vnd:` modules, as an app is offered them.
//!
//! `types/neoc.ast.json` (written by neoc-vm's `luau_decls --ast`) is the tree of every module's declarations, as
//! neoc-vm recorded them: one [`Section`] per module, the last alias of each the type of the module's table
//! (`lib:json` is `lib_json`). This keeps the modules an app is offered, hands their `require` overloads to
//! [`defs`](super::defs), and drops the modules in [`vos_domain::NOT_FOR_APPS`].
//!
//! This reads neoc's file; it does not depend on neoc-vm (`scripts/check-deps.ts` keeps vos-codegen to the domain
//! and the tree's own crate). A file that does not have the expected shape is an error, never a silently thinner
//! definitions file.

use neoc_decl::Section;

/// What the app's definitions take from neoc's modules.
#[derive(Debug, PartialEq, Eq)]
pub struct Offered {
    /// The offered modules, in neoc's order.
    pub modules: Vec<Section>,
    /// `(module, type)` of every offered module, in neoc's order: `("lib:json", "lib_json")`.
    pub overloads: Vec<(String, String)>,
    /// The modules that were left out.
    pub refused: Vec<String>,
}

/// The modules of `types/neoc.ast.json`.
///
/// # Errors
/// The text is not the JSON of the tree.
pub fn read(json: &str) -> Result<Vec<Section>, String> {
    serde_json::from_str(json)
        .map_err(|e| format!("{} is not the tree of neoc's modules: {e}", crate::NEOC_AST))
}

/// `((path: "lib:json") -> lib_json)` -> `("lib:json", "lib_json")`.
pub(crate) fn overload(line: &str) -> Option<(String, String)> {
    let line = line.trim().trim_start_matches('&').trim();
    let rest = line.strip_prefix("((path: \"")?;
    let (module, rest) = rest.split_once("\") -> ")?;
    let ty = rest.strip_suffix(')')?;
    Some((module.to_owned(), ty.to_owned()))
}

/// The modules an app is offered: all of them but `not_for_apps`.
///
/// # Errors
/// A module does not declare the type of its own table, or a module that is not offered to apps is not there:
/// neoc's format changed, or the list of refused modules names a module that is gone.
pub fn offered(modules: &[Section], not_for_apps: &[&str]) -> Result<Offered, String> {
    let mut all = Vec::new();
    for module in modules {
        let table = module.name.replace(':', "_");
        if !module.aliases.iter().any(|alias| alias.name == table) {
            return Err(format!("{} does not declare `{table}`, the type of its table", module.name));
        }
        all.push((module.name.clone(), table));
    }
    for module in not_for_apps {
        if !all.iter().any(|(m, _)| m == module) {
            return Err(format!("{module} is not offered to apps, but neoc has no such module"));
        }
    }

    let refused = |name: &str| not_for_apps.contains(&name);
    Ok(Offered {
        modules: modules.iter().filter(|module| !refused(&module.name)).cloned().collect(),
        overloads: all.into_iter().filter(|(module, _)| !refused(module)).collect(),
        refused: not_for_apps.iter().map(|m| (*m).to_owned()).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Section> {
        read(
            r#"[{"name":"std:env","aliases":[{"name":"std_env","ty":{"table":{}}}]},
                {"name":"lib:test","aliases":[{"name":"lib_test","ty":{"table":{}}}]}]"#,
        )
        .unwrap()
    }

    #[test]
    fn a_refused_module_is_left_out_with_its_overload() {
        let got = offered(&sample(), &["lib:test"]).unwrap();
        assert_eq!(got.modules.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["std:env"]);
        assert_eq!(got.overloads, [("std:env".to_owned(), "std_env".to_owned())]);
        assert_eq!(got.refused, ["lib:test"]);
    }

    #[test]
    fn a_file_of_another_shape_is_an_error() {
        assert!(read("-- no tree\n").unwrap_err().contains("is not the tree of neoc's modules"));
        let mut no_table = sample();
        no_table[1].aliases.clear();
        assert!(offered(&no_table, &[]).unwrap_err().contains("lib:test does not declare `lib_test`"));
        assert!(offered(&sample(), &["lib:gone"]).unwrap_err().contains("neoc has no such module"));
    }
}
