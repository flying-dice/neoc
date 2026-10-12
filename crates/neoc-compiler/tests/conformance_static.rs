//! Data-driven static conformance suite for `lang/CONFORMANCE/static/`.
//!
//! Each `<name>.neoc` case is type-checked with [`neoc_types::check`]; its
//! sibling `<name>.diagnostics` is the set of static-semantics errors the
//! checker must report, one per line (an empty file means the program is
//! well-typed). The expected diagnostics are hand-reasoned from `lang/STATIC.md`
//! — the rule each case exercises is cited in its filename prefix and a leading
//! comment — never copied from compiler output. The spec leads; the impl
//! follows.
//!
//! Like the dynamic runner this uses `libtest-mimic` (`harness = false`) to emit
//! one visible test per case, so a failure names exactly which program diverged.
//! The match is an order-independent multiset comparison: conformance pins
//! *which* diagnostics fire and how many times each, not the order the checker
//! happens to walk the module in — a diagnostic reported twice must be listed
//! twice.
//! A case must also parse with no parser diagnostics — the static layer checks
//! well-formed syntax, so a parse error there is a corpus bug, not a finding.
//!
//! Two extra trials guard the corpus shape: it is non-empty, and every `.neoc`
//! has a sibling `.diagnostics`.

use std::path::{Path, PathBuf};

use libtest_mimic::{Arguments, Failed, Trial};

/// The `lang/CONFORMANCE/static/` directory, resolved from the crate manifest.
fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("lang")
        .join("CONFORMANCE")
        .join("static")
}

/// Every `.neoc` file in the corpus, sorted for a stable iteration order.
fn cases(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("read corpus dir")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "neoc"))
        .collect();
    paths.sort();
    paths
}

/// The non-empty lines of a `.diagnostics` file, sorted — a multiset.
fn expected_multiset(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect();
    lines.sort();
    lines
}

/// The entries of `a` not matched one-for-one by an entry of `b` — multiset
/// difference, so a duplicate in `a` survives one occurrence in `b`.
fn multiset_minus<'a>(a: &'a [String], b: &[String]) -> Vec<&'a String> {
    let mut rest: Vec<&String> = b.iter().collect();
    a.iter()
        .filter(|x| match rest.iter().position(|y| y == x) {
            Some(i) => {
                rest.swap_remove(i);
                false
            }
            None => true,
        })
        .collect()
}

/// Check one case: parse, type-check, and assert the diagnostic multiset equals the
/// committed `.diagnostics` golden. A parser diagnostic fails the trial — the
/// static layer's inputs must be syntactically well-formed.
fn check_case(neoc_path: &Path, case: &str) -> Result<(), Failed> {
    let src = std::fs::read_to_string(neoc_path)
        .map_err(|e| Failed::from(format!("case `{case}`: read .neoc: {e}")))?;
    let parsed = neoc_parser::parse(&src);
    if !parsed.diagnostics.is_empty() {
        let msgs: Vec<&str> = parsed
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect();
        return Err(Failed::from(format!(
            "case `{case}`: source did not parse cleanly (a static case must be well-formed syntax): {msgs:?}"
        )));
    }

    let check = neoc_types::check(&parsed.module);
    let mut got: Vec<String> = check
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    got.sort();

    let diag_path = neoc_path.with_extension("diagnostics");
    let expected_text = std::fs::read_to_string(&diag_path)
        .map_err(|e| Failed::from(format!("case `{case}`: read {}: {e}", diag_path.display())))?;
    let want = expected_multiset(&expected_text);

    if got == want {
        return Ok(());
    }

    let missing = multiset_minus(&want, &got);
    let extra = multiset_minus(&got, &want);
    Err(Failed::from(format!(
        "case `{case}`: diagnostic multiset mismatch\n--- expected but not reported ---\n{missing:#?}\n--- reported but not expected ---\n{extra:#?}"
    )))
}

/// Build the per-case trials plus the corpus-shape guards.
fn collect_trials() -> Vec<Trial> {
    let dir = corpus_dir();
    let cases = cases(&dir);

    let mut trials = Vec::new();

    // Guard: the corpus must be non-empty.
    let n_cases = cases.len();
    let dir_display = dir.display().to_string();
    trials.push(Trial::test("corpus::non_empty", move || {
        if n_cases == 0 {
            Err(Failed::from(format!(
                "no .neoc cases found under {dir_display}"
            )))
        } else {
            Ok(())
        }
    }));

    // Guard: every `.neoc` has a sibling `.diagnostics` so the two halves can't
    // drift.
    let case_paths = cases.clone();
    trials.push(Trial::test(
        "corpus::every_neoc_has_diagnostics",
        move || {
            for neoc in &case_paths {
                let diag = neoc.with_extension("diagnostics");
                if !diag.exists() {
                    let case = neoc.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
                    return Err(Failed::from(format!(
                        "case `{case}`: missing sibling {}",
                        diag.display()
                    )));
                }
            }
            Ok(())
        },
    ));

    for neoc in cases {
        let case = neoc
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("case file stem")
            .to_owned();
        let neoc_path = neoc.clone();
        trials.push(Trial::test(case.clone(), move || {
            check_case(&neoc_path, &case)
        }));
    }

    trials
}

fn main() {
    let args = Arguments::from_args();
    libtest_mimic::run(&args, collect_trials()).exit();
}
