//! Project model: the `neoc.toml` manifest, the fixed on-disk layout, and
//! root discovery (`lang/PROJECT.md`).
//!
//! A project is the unit a build operates on. A lone `.neoc` file needs no
//! project; the model is opt-in and adds nothing by its absence. Discovery
//! walks **up** from a starting path to the first ancestor holding a
//! `neoc.toml`; when none is found, callers fall back to single-file
//! resolution (the importing file's own directory), byte-identical to the
//! behaviour before projects existed.

use std::path::{Path, PathBuf};

use crate::{Error, Target};

/// The manifest file that roots a project.
const MANIFEST: &str = "neoc.toml";

/// A discovered project: its root directory, declared identity and default
/// target, and the derived source root and entry module.
///
/// All paths are absolute when discovery starts from an absolute path; they
/// are kept verbatim (not canonicalised) so they read back as the layout the
/// user sees on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// The directory containing `neoc.toml`.
    pub root: PathBuf,
    /// The project's name, from `[package].name`.
    pub name: String,
    /// The default build target, from `[package].target`. A build may override
    /// it.
    pub target: Target,
    /// The source root, `<root>/src/main/neoc`. All module resolution anchors
    /// here.
    pub source_root: PathBuf,
    /// The entry module, `<source_root>/main.neoc`.
    pub entry: PathBuf,
}

impl Project {
    /// The build-output directory for `target`: `<root>/src/main/<target>`.
    ///
    /// `target` is taken as an argument rather than read from `self.target` so
    /// a build that overrides the manifest target writes to the matching
    /// output directory.
    #[must_use]
    pub fn output_dir(&self, target: Target) -> PathBuf {
        self.root.join("src").join("main").join(target.to_string())
    }

    /// The SQL-input directory: `<root>/src/main/sql`. SQL-derived modules read
    /// from here and write into the source root.
    #[must_use]
    pub fn sql_dir(&self) -> PathBuf {
        self.root.join("src").join("main").join("sql")
    }
}

/// The `[package]` table of a manifest, as parsed.
#[derive(serde::Deserialize)]
struct Manifest {
    package: Package,
}

#[derive(serde::Deserialize)]
struct Package {
    name: String,
    target: String,
}

/// Discovers the project rooted at the first ancestor of `start` that contains
/// a `neoc.toml`.
///
/// Walks **up** the directory tree from `start`'s directory (or from `start`
/// itself when it is a directory), testing each ancestor for the manifest and
/// stopping at the first found. Returns `Ok(None)` when no ancestor holds a
/// manifest — the caller then falls back to single-file resolution.
///
/// # Arguments
///
/// * `start` — a file or directory to begin the upward walk from. When it is a
///   file, the walk starts at its parent directory.
///
/// # Errors
///
/// Returns [`Error::Project`] only when a manifest *is* found but is malformed:
/// not valid TOML, missing `[package]`, missing `name`, or carrying a `target`
/// that is neither `"lua"` nor `"js"`. A missing manifest is not an error.
pub fn discover_project(start: &Path) -> Result<Option<Project>, Error> {
    let mut dir: &Path = if start.is_dir() {
        start
    } else {
        start.parent().unwrap_or(start)
    };

    // Walk up to the filesystem root, Cargo-style — the nearest `neoc.toml`
    // wins, and reaching the root with none found simply means "not a project".
    // The walk is intentionally unbounded; bounding it would diverge from the
    // established manifest-discovery convention.
    loop {
        let manifest = dir.join(MANIFEST);
        if manifest.is_file() {
            return load_project(dir, &manifest).map(Some);
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => return Ok(None),
        }
    }
}

/// Reads and validates the manifest at `manifest_path`, building the [`Project`]
/// rooted at `root`. All `toml` errors are mapped to a neoc-shaped
/// [`Error::Project`] at this boundary so no host parser type escapes.
fn load_project(root: &Path, manifest_path: &Path) -> Result<Project, Error> {
    let text = std::fs::read_to_string(manifest_path)
        .map_err(|e| Error::Project(format!("cannot read manifest `{MANIFEST}`: {e}")))?;
    let manifest: Manifest = toml::from_str(&text)
        // The full Display carries the line/column and a source snippet ("TOML
        // parse error at line N, column M"); `.message()` drops the location.
        .map_err(|e| Error::Project(format!("invalid `{MANIFEST}`: {e}")))?;

    let target: Target = manifest.package.target.parse().map_err(|_| {
        Error::Project(format!(
            "`{MANIFEST}` has an unknown target `{}` (expected `lua`, `js`, `ts`, or `luau`)",
            manifest.package.target
        ))
    })?;

    let source_root = root.join("src").join("main").join("neoc");
    let entry = source_root.join("main.neoc");
    Ok(Project {
        root: root.to_path_buf(),
        name: manifest.package.name,
        target,
        source_root,
        entry,
    })
}

/// The directory module resolution anchors against for `entry`.
///
/// Returns the project's source root when `entry` lies in a project (an
/// ancestor holds a `neoc.toml`), and `entry`'s own parent directory otherwise
/// — the single-file rule, byte-identical to the value used before projects
/// existed. A malformed manifest is treated as no manifest here (resolution
/// must not fail on it; the build/check path surfaces the manifest error
/// separately), so resolution degrades gracefully to the single-file anchor.
#[must_use]
pub fn resolution_root(entry: &Path) -> PathBuf {
    match discover_project(entry) {
        Ok(Some(project)) => project.source_root,
        _ => entry
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A unique temp directory for one test, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let mut p = std::env::temp_dir();
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            p.push(format!("neoc-project-{tag}-{nanos}"));
            fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_manifest(root: &Path, body: &str) {
        fs::write(root.join("neoc.toml"), body).unwrap();
    }

    #[test]
    fn walk_up_finds_root_from_nested_file() {
        let tmp = TempDir::new("walk-up");
        let root = tmp.path();
        write_manifest(root, "[package]\nname = \"app\"\ntarget = \"js\"\n");
        let nested = root.join("src").join("main").join("neoc").join("a");
        fs::create_dir_all(&nested).unwrap();
        let file = nested.join("b.neoc");
        fs::write(&file, "").unwrap();

        let project = discover_project(&file).unwrap().expect("found project");
        assert_eq!(project.root, root);
        assert_eq!(project.name, "app");
        assert_eq!(project.target, Target::Js);
    }

    #[test]
    fn walk_up_returns_none_when_no_manifest() {
        let tmp = TempDir::new("no-manifest");
        let nested = tmp.path().join("a").join("b");
        fs::create_dir_all(&nested).unwrap();
        let file = nested.join("lone.neoc");
        fs::write(&file, "").unwrap();

        assert!(discover_project(&file).unwrap().is_none());
    }

    #[test]
    fn malformed_target_is_neoc_termed_error() {
        let tmp = TempDir::new("bad-target");
        write_manifest(tmp.path(), "[package]\nname = \"x\"\ntarget = \"wasm\"\n");
        let err = discover_project(tmp.path()).unwrap_err();
        #[allow(irrefutable_let_patterns)]
        let Error::Project(msg) = err else {
            panic!("expected Error::Project");
        };
        assert!(msg.contains("unknown target"), "{msg}");
        assert!(msg.contains("wasm"), "{msg}");
        // No host parser jargon leaks.
        assert!(!msg.contains("toml::"), "{msg}");
    }

    #[test]
    fn invalid_toml_error_carries_location_without_host_jargon() {
        let tmp = TempDir::new("bad-toml");
        // Missing value after `=` — a syntax error at a specific line/column.
        write_manifest(tmp.path(), "[package]\nname =\n");
        let err = discover_project(tmp.path()).unwrap_err();
        #[allow(irrefutable_let_patterns)]
        let Error::Project(msg) = err else {
            panic!("expected Error::Project");
        };
        // The location is surfaced, and no host parser jargon leaks.
        assert!(msg.contains("line"), "expected a location in: {msg}");
        assert!(!msg.contains("toml::"), "{msg}");
    }

    #[test]
    fn missing_package_table_is_error() {
        let tmp = TempDir::new("no-package");
        write_manifest(tmp.path(), "name = \"x\"\n");
        let err = discover_project(tmp.path()).unwrap_err();
        assert!(matches!(err, Error::Project(_)));
    }

    #[test]
    fn invalid_toml_is_error() {
        let tmp = TempDir::new("bad-toml");
        write_manifest(tmp.path(), "this is not = = toml");
        let err = discover_project(tmp.path()).unwrap_err();
        assert!(matches!(err, Error::Project(_)));
    }

    #[test]
    fn layout_derivation() {
        let tmp = TempDir::new("layout");
        write_manifest(tmp.path(), "[package]\nname = \"app\"\ntarget = \"lua\"\n");
        let project = discover_project(tmp.path()).unwrap().unwrap();
        let root = tmp.path();
        assert_eq!(project.source_root, root.join("src/main/neoc"));
        assert_eq!(project.entry, root.join("src/main/neoc/main.neoc"));
        assert_eq!(project.output_dir(Target::Lua), root.join("src/main/lua"));
        assert_eq!(project.output_dir(Target::Js), root.join("src/main/js"));
        assert_eq!(project.sql_dir(), root.join("src/main/sql"));
    }

    #[test]
    fn resolution_root_in_project_is_source_root() {
        let tmp = TempDir::new("res-in");
        write_manifest(tmp.path(), "[package]\nname = \"app\"\ntarget = \"lua\"\n");
        let src = tmp.path().join("src/main/neoc");
        fs::create_dir_all(&src).unwrap();
        let entry = src.join("main.neoc");
        fs::write(&entry, "").unwrap();
        assert_eq!(resolution_root(&entry), src);
    }

    #[test]
    fn resolution_root_outside_project_is_parent() {
        let tmp = TempDir::new("res-out");
        let dir = tmp.path().join("loose");
        fs::create_dir_all(&dir).unwrap();
        let entry = dir.join("main.neoc");
        fs::write(&entry, "").unwrap();
        assert_eq!(resolution_root(&entry), dir);
    }

    #[test]
    fn unknown_keys_ignored() {
        let tmp = TempDir::new("unknown-keys");
        write_manifest(
            tmp.path(),
            "[package]\nname = \"app\"\ntarget = \"js\"\nversion = \"1.0\"\n[extra]\nfoo = 1\n",
        );
        let project = discover_project(tmp.path()).unwrap().unwrap();
        assert_eq!(project.name, "app");
        assert_eq!(project.target, Target::Js);
    }
}
