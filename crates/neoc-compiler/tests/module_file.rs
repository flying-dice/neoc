//! `module_file` is the shared `use`-path → file rule; the compiler and the
//! language server both resolve modules through it.

use std::path::{Path, PathBuf};

use neoc_compiler::module_file;

#[test]
fn maps_a_nested_module_path_to_its_file() {
    // `use math::vec::Vec3` — the last segment is the item, the rest the module.
    let path = vec!["math".to_string(), "vec".to_string(), "Vec3".to_string()];
    assert_eq!(
        module_file(Path::new("/root"), &path),
        Some(PathBuf::from("/root/math/vec.neoc"))
    );
}

#[test]
fn a_two_segment_path_is_a_top_level_sibling() {
    // `use math::Vec3` → <root>/math.neoc
    let path = vec!["math".to_string(), "Vec3".to_string()];
    assert_eq!(
        module_file(Path::new("/root"), &path),
        Some(PathBuf::from("/root/math.neoc"))
    );
}

#[test]
fn a_single_segment_has_no_module_file() {
    // `use Item` — no module qualifier, nothing to resolve to a sibling.
    assert_eq!(module_file(Path::new("/root"), &["Item".to_string()]), None);
}
