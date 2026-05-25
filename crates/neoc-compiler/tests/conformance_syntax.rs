//! Data-driven syntax conformance suite for `lang/CONFORMANCE/syntax/`.
//!
//! Parses each case directly against `neoc_grammar::LANGUAGE` — the tree-sitter
//! grammar, the parser the spec in `lang/SYNTAX.md` is canonical over — and
//! asserts accept/reject at the CST level, below the typed-AST adapter.
//!
//! Two case shapes:
//!
//! - `<name>.neoc` — must parse with **no** error node: a syntactically valid
//!   program per `SYNTAX.md`.
//! - `<name>.reject` — a source the parser must reject: the tree must contain an
//!   error or missing node. Its sibling `<name>.reject.expected` names the error
//!   category in prose (e.g. `bare struct literal in condition position`). The
//!   category is documentation — conformance pins *that* the source is rejected,
//!   not the exact recovery shape, which is parser-dependent.
//!
//! Like the other runners this uses `libtest-mimic` (`harness = false`) for one
//! visible test per case. Two extra trials guard the corpus shape: it is
//! non-empty, and every `.reject` has a sibling `.reject.expected`.

use std::path::{Path, PathBuf};

use libtest_mimic::{Arguments, Failed, Trial};
use tree_sitter::Parser;

/// Parse `source` against the neoc grammar and return whether the tree contains
/// an error or missing node (tree-sitter's signal that the input was rejected).
fn has_error(source: &str) -> bool {
    let mut parser = Parser::new();
    parser
        .set_language(&neoc_grammar::LANGUAGE.into())
        .expect("load neoc grammar");
    let tree = parser
        .parse(source, None)
        .expect("tree-sitter never returns None here");
    tree.root_node().has_error()
}

/// The s-expression of the parse tree, for failure messages.
fn sexp(source: &str) -> String {
    let mut parser = Parser::new();
    parser
        .set_language(&neoc_grammar::LANGUAGE.into())
        .expect("load neoc grammar");
    let tree = parser
        .parse(source, None)
        .expect("tree-sitter never returns None here");
    tree.root_node().to_sexp()
}

/// The `lang/CONFORMANCE/syntax/` directory, resolved from the crate manifest.
fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("lang")
        .join("CONFORMANCE")
        .join("syntax")
}

/// All entries in the corpus with the given extension, sorted.
fn files_with_ext(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("read corpus dir")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == ext))
        .collect();
    paths.sort();
    paths
}

/// Build the per-case trials plus the corpus-shape guards.
fn collect_trials() -> Vec<Trial> {
    let dir = corpus_dir();
    let accepts = files_with_ext(&dir, "neoc");
    let rejects = files_with_ext(&dir, "reject");

    let mut trials = Vec::new();

    // Guard: the corpus must have both accept and reject cases.
    let n_accept = accepts.len();
    let n_reject = rejects.len();
    let dir_display = dir.display().to_string();
    trials.push(Trial::test("corpus::non_empty", move || {
        if n_accept == 0 || n_reject == 0 {
            Err(Failed::from(format!(
                "corpus under {dir_display} needs both accept (.neoc) and reject (.reject) cases; got {n_accept} accept, {n_reject} reject"
            )))
        } else {
            Ok(())
        }
    }));

    // Guard: every `.reject` names its expected category.
    let reject_paths = rejects.clone();
    trials.push(Trial::test(
        "corpus::every_reject_has_expected",
        move || {
            for r in &reject_paths {
                let expected = r.with_extension("reject.expected");
                if !expected.exists() {
                    let case = r.file_name().and_then(|s| s.to_str()).unwrap_or("?");
                    return Err(Failed::from(format!(
                        "case `{case}`: missing sibling {}",
                        expected.display()
                    )));
                }
            }
            Ok(())
        },
    ));

    for neoc in accepts {
        let case = neoc
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("case file stem")
            .to_owned();
        trials.push(Trial::test(format!("{case}::accept"), move || {
            let src = std::fs::read_to_string(&neoc)
                .map_err(|e| Failed::from(format!("case `{case}`: read .neoc: {e}")))?;
            if has_error(&src) {
                Err(Failed::from(format!(
                    "case `{case}`: expected a clean parse, but the tree has an error node:\n{}",
                    sexp(&src)
                )))
            } else {
                Ok(())
            }
        }));
    }

    for reject in rejects {
        // `<name>.reject` → stem is `<name>`.
        let case = reject
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("case file stem")
            .to_owned();
        let expected_path = reject.with_extension("reject.expected");
        trials.push(Trial::test(format!("{case}::reject"), move || {
            let src = std::fs::read_to_string(&reject)
                .map_err(|e| Failed::from(format!("case `{case}`: read .reject: {e}")))?;
            let category = std::fs::read_to_string(&expected_path)
                .map_err(|e| {
                    Failed::from(format!(
                        "case `{case}`: read {}: {e}",
                        expected_path.display()
                    ))
                })?
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty() && !l.starts_with("//"))
                .unwrap_or("")
                .to_owned();
            if has_error(&src) {
                Ok(())
            } else {
                Err(Failed::from(format!(
                    "case `{case}`: expected the parser to reject this ({category}), but it parsed clean:\n{}",
                    sexp(&src)
                )))
            }
        }));
    }

    trials
}

fn main() {
    let args = Arguments::from_args();
    libtest_mimic::run(&args, collect_trials()).exit();
}
