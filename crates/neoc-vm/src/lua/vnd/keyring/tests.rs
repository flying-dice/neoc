//! `vnd:keyring` against `keyring-core`'s mock store: no test reaches the credential store of whoever runs them.
//! `the_platform_store_keeps_a_credential` is the one test against a real store, and is ignored.

use mlua::Value;

use super::*;
use crate::lua::sandbox::make_engine;

/// Run `script` in a new engine.
async fn run<R: mlua::FromLuaMulti>(script: &str) -> R {
    make_engine()
        .expect("engine")
        .load(script)
        .call_async(())
        .await
        .unwrap_or_else(|e| panic!("the script failed: {e}"))
}

/// Run `script`, which returns the `(value, err)` of a failed call: the error string.
async fn error_of(script: &str) -> String {
    let (value, err): (Value, Option<String>) = run(script).await;
    assert!(value.is_nil(), "the call succeeded: {value:?}");
    err.expect("an error string")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_password_is_kept_replaced_and_deleted() {
    let (first, second, deleted, after): (String, String, bool, Value) = run(r#"
        local entry = assert(require("vnd:keyring").mock():entry("example.com", "ada"))
        assert(entry:set_password("first") == true)
        local first = assert(entry:get_password())
        assert(entry:set_password("second"))
        local second = assert(entry:get_password())
        local deleted = assert(entry:delete_credential())
        return first, second, deleted, (entry:get_password())
        "#)
    .await;
    assert_eq!(first, "first");
    assert_eq!(second, "second");
    assert!(deleted);
    assert!(after.is_nil());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_credential_is_an_answer_and_not_an_error() {
    let (password, password_err, secret, secret_err, deleted, delete_err): (
        Value,
        Value,
        Value,
        Value,
        bool,
        Value,
    ) = run(r#"
        local entry = assert(require("vnd:keyring").mock():entry("example.com", "nobody"))
        local password, password_err = entry:get_password()
        local secret, secret_err = entry:get_secret()
        local deleted, delete_err = entry:delete_credential()
        return password, password_err, secret, secret_err, deleted, delete_err
        "#)
    .await;
    assert!(password.is_nil() && password_err.is_nil());
    assert!(secret.is_nil() && secret_err.is_nil());
    assert!(!deleted, "there was nothing to delete");
    assert!(delete_err.is_nil());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_secret_is_the_bytes_that_were_stored() {
    // the engine outlives the string it returned
    let lua = make_engine().expect("engine");
    let (secret, as_text_err): (mlua::LuaString, String) = lua
        .load(
            r#"
            local entry = assert(require("vnd:keyring").mock():entry("example.com", "key"))
            assert(entry:set_secret("\0\255binary\0"))
            local secret = assert(entry:get_secret())
            local password, err = entry:get_password()
            assert(password == nil)
            return secret, err
            "#,
        )
        .call_async(())
        .await
        .expect("the script");
    assert_eq!(&*secret.as_bytes(), b"\0\xffbinary\0");
    assert!(
        as_text_err.starts_with("keyring.get_password: bad_encoding: "),
        "{as_text_err}"
    );
    assert!(
        !as_text_err.contains("binary"),
        "the secret is in the error"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_entries_of_one_name_are_one_credential_and_stores_are_apart() {
    let (through_other, in_other_store): (String, Value) = run(r#"
        local keyring = require("vnd:keyring")
        local store = keyring.mock()
        assert(assert(store:entry("example.com", "ada")):set_password("shared"))
        local through_other = assert(assert(store:entry("example.com", "ada")):get_password())
        local elsewhere = assert(keyring.mock():entry("example.com", "ada"))
        return through_other, (elsewhere:get_password())
        "#)
    .await;
    assert_eq!(through_other, "shared");
    assert!(in_other_store.is_nil(), "a mock store is its own");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_entry_says_what_it_was_made_from() {
    let (service, user): (String, String) = run(
        r#"return assert(require("vnd:keyring").mock():entry("example.com", "ada")):get_specifiers()"#,
    )
    .await;
    assert_eq!((service.as_str(), user.as_str()), ("example.com", "ada"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn search_returns_the_entries_that_match() {
    let (all, of_ada, password): (usize, usize, String) = run(r#"
        local store = require("vnd:keyring").mock()
        assert(assert(store:entry("example.com", "ada")):set_password("a"))
        assert(assert(store:entry("example.com", "grace")):set_password("g"))
        assert(assert(store:entry("other.org", "ada")):set_password("o"))
        local all = assert(store:search())
        local of_ada = assert(store:search({ service = "example", user = "ada" }))
        return #all, #of_ada, assert(of_ada[1]:get_password())
        "#)
    .await;
    assert_eq!(all, 3);
    assert_eq!(of_ada, 1);
    assert_eq!(password, "a");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_argument_that_is_not_text_is_refused_before_the_store() {
    let service = error_of(r#"return require("vnd:keyring").mock():entry("\255", "ada")"#).await;
    assert_eq!(
        service,
        "keyring.entry: invalid_input: the service is not UTF-8 text"
    );

    let password = error_of(
        r#"return assert(require("vnd:keyring").mock():entry("example.com", "ada")):set_password("\255")"#,
    )
    .await;
    assert_eq!(
        password,
        "keyring.set_password: invalid_input: the password is not UTF-8 text"
    );

    let spec = error_of(r#"return require("vnd:keyring").mock():search({ service = 1 })"#).await;
    assert_eq!(
        spec,
        "keyring.search: invalid_input: `spec` is a table of text under text"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn error_kind_reads_this_modules_errors_only() {
    let (ours, theirs, none): (String, Value, Value) = run(
        r#"
        local keyring = require("vnd:keyring")
        local _, err = keyring.mock():entry("\255", "ada")
        return keyring.error_kind(err), keyring.error_kind("fs.read: not_found: x"), keyring.error_kind(nil)
        "#,
    )
    .await;
    assert_eq!(ours, "invalid_input");
    assert!(theirs.is_nil());
    assert!(none.is_nil());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn error_message_is_what_follows_the_kind_of_this_modules_errors_only() {
    let (err, message, empty, theirs, none): (String, String, String, Value, Value) = run(r#"
        local keyring = require("vnd:keyring")
        local _, err = keyring.mock():entry("\255", "ada")
        assert(keyring.error_message("keyring.store: nonsense: x") == nil)
        assert(keyring.error_message("something else") == nil)
        return err, keyring.error_message(err), keyring.error_message("keyring.store: no_store"),
            keyring.error_message("fs.read: not_found: x"), keyring.error_message(nil)
        "#)
    .await;
    assert!(!message.is_empty(), "{err}");
    assert_eq!(
        err.strip_prefix("keyring.entry: invalid_input: "),
        Some(message.as_str())
    );
    assert_eq!(empty, "");
    assert!(theirs.is_nil());
    assert!(none.is_nil());
}

#[test]
fn every_kind_is_in_the_docs_of_error_kind() {
    let lua = make_engine().expect("engine");
    let decls = declare(&lua)
        .expect("declare")
        .luau_decls()
        .expect("declarations");
    for kind in Kind::ALL {
        assert!(
            decls.contains(&format!("`{}`", kind.as_str())),
            "`{}` is not documented",
            kind.as_str()
        );
    }
}

#[test]
fn the_environment_chooses_the_process_mock_and_refuses_anything_else() {
    let first = chosen_store(Some(MOCK)).expect("a mock store");
    let again = chosen_store(Some(MOCK)).expect("a mock store");
    assert!(
        Arc::ptr_eq(&first.inner, &again.inner),
        "one mock store for the process"
    );
    assert_eq!(
        first.inner.vendor(),
        mock_store().expect("mock").inner.vendor()
    );

    let refused = chosen_store(Some("memory")).err().expect("an error");
    assert_eq!(
        refused.to_string(),
        "invalid_input: NEOC_KEYRING is `memory`: the one value it takes is `mock`"
    );
}

/// The real store of this machine: `cargo test -p neoc-vm -- --ignored the_platform_store` (macOS may ask to allow
/// the test binary; a Linux machine needs a Secret Service). It makes one credential and deletes it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "writes to the credential store of whoever runs it"]
async fn the_platform_store_keeps_a_credential() {
    let (kept, deleted, after): (String, bool, Value) = run(r#"
        local store = assert(require("vnd:keyring").store())
        local entry = assert(store:entry("neoc-vm.tests", "the_platform_store_keeps_a_credential"))
        assert(entry:set_password("kept"))
        local kept = assert(entry:get_password())
        local deleted = assert(entry:delete_credential())
        return kept, deleted, (entry:get_password())
        "#)
    .await;
    assert_eq!(kept, "kept");
    assert!(deleted);
    assert!(after.is_nil());
}
