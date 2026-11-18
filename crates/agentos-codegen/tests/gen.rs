#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
use std::process::Command;

#[test]
fn typescript_has_the_ui_types_with_serde_shapes() {
    let ts = agentos_codegen::typescript();
    println!("{ts}");
    assert!(ts.contains("export type UiNode ="));
    assert!(ts.contains("\"type\": \"button\""), "serde tag preserved");
    assert!(ts.contains("export type AppId = string;"));
}

#[test]
fn generated_luau_typechecks_under_strict_and_catches_misuse() {
    let luau = agentos_codegen::luau();
    println!("{luau}");
    assert!(luau.contains("export type UiNode ="));
    let dir = std::env::temp_dir().join(format!("luaugen-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // an app author's file using the generated types
    let usage = format!(
        "{luau}\nlocal ok: UiNode = {{ type = \"stack\", direction = \"column\", children = {{ {{ type = \"button\", id = \"go\", label = \"Go\" }}, {{ type = \"text\", text = \"hi\" }} }} }}\nreturn ok\n"
    );
    std::fs::write(dir.join("ok.luau"), &usage).unwrap();
    std::fs::write(
        dir.join("bad.luau"),
        usage.replace("type = \"text\", text = \"hi\"", "type = \"text\", txt = \"hi\""),
    )
    .unwrap();
    let Ok(good) = Command::new("luau-analyze").arg(dir.join("ok.luau")).output() else {
        eprintln!("skipped: luau-analyze not installed");
        return;
    };
    assert!(
        good.status.success(),
        "{}{}",
        String::from_utf8_lossy(&good.stdout),
        String::from_utf8_lossy(&good.stderr)
    );
    let bad = Command::new("luau-analyze").arg(dir.join("bad.luau")).output().unwrap();
    assert!(!bad.status.success(), "a wrongly-shaped node must be a type error");
    let _ = std::fs::remove_dir_all(dir);
}
