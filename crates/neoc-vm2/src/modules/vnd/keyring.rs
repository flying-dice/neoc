//! The operating system's credential store: `vnd:keyring` (the `keyring` crate and its `keyring-core`).
//!
//! ```lua
//! local keyring = require("vnd:keyring")
//!
//! local store = assert(keyring.store())
//! local entry = assert(store:entry("example.com", "ada"))
//! assert(entry:set_password("correct horse battery staple"))
//! print(entry:get_password())        --> correct horse battery staple
//! assert(entry:delete_credential())
//! print(entry:get_password())        --> nil nil
//! ```
//!
//! Which store: `store()` is the platform's own, chosen by the `keyring` crate: Keychain Services on macOS (the
//! login keychain), the Credential Manager on Windows, the Secret Service on Linux and the other Unixes (GNOME
//! Keyring, KWallet). A machine without one (a headless Linux box with no Secret Service) answers `no_store`.
//! `mock()` is `keyring-core`'s mock store: in memory, gone with the process, for tests. With `NEOC_KEYRING=mock`
//! in the environment `store()` is one mock store shared by the whole process, so a program can be run without it
//! touching the real store.
//!
//! Entries: a credential is named by a `service` and a `user`, both text. `Store:entry` makes an `Entry` for that
//! pair and touches nothing: the credential need not exist. Its secret is text (`get_password`, `set_password`) or
//! bytes (`get_secret`, `set_secret`). Reading a credential that is not there is `(nil, nil)`, and deleting one is
//! `false`: neither is an error.
//!
//! Errors: a call returns `(value, nil)` or `(nil, err)`, where `err.kind` is stable and `err.message` is the
//! store's own wording where it answered (for a log; it differs between platforms and versions). No error holds a
//! secret: the bytes a store hands back with an error are dropped.
//!
//! Waiting: every call that reaches the store runs on the blocking pool and the script waits for it: the platform
//! may ask the person (macOS shows a dialog before it hands a credential made by another program to this one) and
//! the call takes as long as they do.
//!
//! What reaching the store means: a script given this module reads and replaces any credential the platform lets
//! this process have, which on most machines is every credential of the user it runs as. Nothing here narrows
//! that, as nothing narrows `std:fs`: a script that is given this module is trusted with the user's secrets.
//!
//! Ported from neoc-vm's `vnd:keyring`. What changed on purpose:
//! - A failure is the typed `Error` (the same kinds: `invalid_input`, `no_store`, `no_access`, `not_found`,
//!   `ambiguous`, `bad_encoding`, `bad_data`, `unsupported`, `platform`, `other`), so `error_kind(err)` and
//!   `error_message(err)` are gone: read `err.kind` and `err.message`. `ambiguous` says how many `matches`.
//! - A service, a user or a password that is not UTF-8 text, and a `spec` that is not text under text, are refused
//!   as arguments; they were the returned error `invalid_input`.
//! - `set_password` and `set_secret` still answer `true`; `mock()` still raises in the (unseen) case that the mock
//!   store cannot be made, now with the typed error.

#[mlua_ir::module("vnd:keyring")]
pub mod keyring {
    use std::collections::HashMap;
    use std::sync::{Arc, OnceLock};

    use keyring_core::{CredentialStore, Entry as Credential};
    use mlua_ir::{Bytes, LuaError, Object, Raise};
    use serde::Serialize;

    /// The environment variable that makes `store()` a mock store ([`MOCK`] is its one value).
    const CHOICE: &str = "NEOC_KEYRING";
    pub(super) const MOCK: &str = "mock";

    /// Why a call on the credential store failed. No field holds a secret.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum Error {
        /// The store refused an argument: a service or user it does not accept, a value longer than it keeps.
        #[error("{reason}")]
        InvalidInput {
            /// What was refused, and why.
            reason: String,
        },
        /// This machine has no credential store that can be used: the platform has none, or it could not be
        /// opened (a Linux machine with no Secret Service running, say).
        #[error("{reason}")]
        NoStore {
            /// Why there is none.
            reason: String,
        },
        /// The store is there and would not give access: it is locked, or the person refused.
        #[error("the credential store would not give access: {reason}")]
        NoAccess {
            /// What the store said.
            reason: String,
        },
        /// The credential an entry stands for is gone. Only an entry that `search` returned says this (it is one
        /// credential): for an entry made from a service and a user, "no credential" is an answer, not an error.
        #[error("the credential is no longer in the store")]
        NotFound,
        /// More than one credential in the store matches the entry's service and user.
        #[error("{matches} credentials in the store match this service and user")]
        Ambiguous {
            /// How many match.
            matches: u32,
        },
        /// The credential's secret is not UTF-8 text: read it with `get_secret`.
        #[error("the secret is not UTF-8 text: read it with `get_secret`")]
        BadEncoding,
        /// What the store holds could not be read: the secret, or the store itself, is not in the store's format.
        #[error("{reason}")]
        BadData {
            /// What could not be read, and why.
            reason: String,
        },
        /// The store does not do what was asked (`search`, on a store that has none).
        #[error("{reason}")]
        Unsupported {
            /// What the store said.
            reason: String,
        },
        /// The platform's store failed.
        #[error("{reason}")]
        Platform {
            /// The platform's own words.
            reason: String,
        },
        /// Anything else, reported and never hidden.
        #[error("{reason}")]
        Other {
            /// What went wrong.
            reason: String,
        },
    }

    impl Error {
        /// The platform's store could not be opened, and why.
        fn no_store(why: &keyring_core::Error) -> Self {
            Error::NoStore { reason: format!("this machine has no credential store that can be used: {why}") }
        }
    }

    impl From<keyring_core::Error> for Error {
        fn from(error: keyring_core::Error) -> Self {
            use keyring_core::Error as Store;
            match error {
                Store::Invalid(what, why) => Error::InvalidInput { reason: format!("`{what}` {why}") },
                Store::TooLong(what, limit) => Error::InvalidInput {
                    reason: format!("`{what}` is longer than the {limit} characters this store keeps"),
                },
                Store::NoDefaultStore => {
                    Error::NoStore { reason: "this machine has no credential store that can be used".to_owned() }
                }
                Store::NoStorageAccess(why) => Error::NoAccess { reason: why.to_string() },
                Store::NoEntry => Error::NotFound,
                // the entries are left out: their `Debug` is the store's, and names what the script already knows
                Store::Ambiguous(matches) => Error::Ambiguous { matches: u32::try_from(matches.len()).unwrap_or(u32::MAX) },
                // the bytes that came with these two are the secret: dropped
                Store::BadEncoding(_) => Error::BadEncoding,
                Store::BadDataFormat(_, why) => {
                    Error::BadData { reason: format!("the secret is not in the store's format: {why}") }
                }
                Store::BadStoreFormat(why) => {
                    Error::BadData { reason: format!("the credential store could not be read: {why}") }
                }
                Store::NotSupportedByStore(why) => Error::Unsupported { reason: why },
                Store::PlatformFailure(why) => Error::Platform { reason: why.to_string() },
                other => Error::Other { reason: other.to_string() },
            }
        }
    }

    /// A credential store: the platform's, or a mock.
    #[derive(Clone, Object)]
    pub struct Store {
        pub(super) inner: Arc<CredentialStore>,
    }

    /// A credential's name in a store (or, from `search`, one credential of it).
    #[derive(Clone, Object)]
    pub struct Entry {
        inner: Arc<Credential>,
    }

    /// The store `store()` hands out for a value of [`CHOICE`] (`None` when it is not set): the platform's, or
    /// the process's mock when the environment asks for one.
    pub(super) fn chosen_store(choice: Option<&str>) -> Result<Store, Error> {
        match choice {
            None | Some("") => platform_store(),
            Some(MOCK) => process_mock(),
            Some(other) => Err(Error::InvalidInput {
                reason: format!("{CHOICE} is `{other}`: the one value it takes is `{MOCK}`"),
            }),
        }
    }

    /// The platform's store. The `keyring` crate opens it once per process and keeps the outcome, so a machine
    /// whose store could not be opened (no Secret Service was running) says so until the process is restarted.
    fn platform_store() -> Result<Store, Error> {
        match ::keyring::Entry::store_status() {
            // the crate sets the store it opened as `keyring-core`'s default; nothing here sets another
            Ok(()) => keyring_core::get_default_store()
                .map(|inner| Store { inner })
                .ok_or_else(|| Error::from(keyring_core::Error::NoDefaultStore)),
            Err(why) => Err(Error::no_store(why)),
        }
    }

    /// The one mock store `NEOC_KEYRING=mock` gives every script of the process.
    fn process_mock() -> Result<Store, Error> {
        static SHARED: OnceLock<Store> = OnceLock::new();
        if let Some(store) = SHARED.get() {
            return Ok(store.clone());
        }
        let made = new_mock()?;
        Ok(SHARED.get_or_init(|| made).clone())
    }

    pub(super) fn new_mock() -> Result<Store, Error> {
        let inner: Arc<CredentialStore> = keyring_core::mock::Store::new()?;
        Ok(Store { inner })
    }

    /// Runs `job` on the blocking pool and waits for it: a store call is the platform's, and may wait for the
    /// person.
    async fn blocking<F, R>(job: F) -> Result<R, Error>
    where
        F: FnOnce() -> Result<R, Error> + Send + 'static,
        R: Send + 'static,
    {
        // Do nothing on the first poll. mlua polls a host call once before it tries to yield, and where Luau
        // refuses the yield (a `gsub` callback, a metamethod) it drops the call after that poll: a job started now
        // would write to the store although the script was told the call failed (`std:fs` has the same rule).
        tokio::task::yield_now().await;
        tokio::task::spawn_blocking(job)
            .await
            .unwrap_or_else(|e| Err(Error::Other { reason: format!("the credential store job failed: {e}") }))
    }

    /// A read for which "the store has no such credential" is the answer `nil`.
    fn found<R>(read: Result<R, keyring_core::Error>) -> Result<Option<R>, Error> {
        match read {
            Ok(value) => Ok(Some(value)),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// The operating system's credential store: Keychain Services on macOS (the login keychain), the Credential
    /// Manager on Windows, the Secret Service on Linux and the other Unixes. A machine that has none, or whose
    /// store cannot be opened, is `no_store`. With `NEOC_KEYRING=mock` in the environment it is a mock store
    /// instead, one for the whole process (see `mock`); any other value of it is `invalid_input`. A script given
    /// this module is trusted with the secrets of the user it runs as.
    #[host("Wait")]
    pub async fn store() -> Result<Store, Error> {
        blocking(|| chosen_store(std::env::var(CHOICE).ok().as_deref())).await
    }

    /// A new mock store, for tests: in memory, its own, and gone with the process. It takes any service and
    /// user, and `search` matches `service` and `user` as substrings.
    ///
    /// ```luau
    /// local entry = assert(keyring.mock():entry("example.com", "ada"))
    /// assert(entry:set_password("secret"))
    /// assert(entry:get_password() == "secret")
    /// assert(entry:delete_credential() == true)
    /// assert(entry:get_password() == nil)
    /// ```
    pub fn mock() -> Result<Store, Raise<Error>> {
        new_mock().map_err(Raise)
    }

    #[mlua_ir::object]
    impl Store {
        /// The entry for a service and a user in this store. Nothing is read or written: the credential need not
        /// exist. What a store accepts as a service or a user is its own (`invalid_input`).
        #[host("Wait")]
        pub async fn entry(&self, service: String, user: String) -> Result<Entry, Error> {
            let store = self.inner.clone();
            blocking(move || {
                let entry = Credential::new_in_store(&store, &service, &user)?;
                Ok(Entry { inner: Arc::new(entry) })
            })
            .await
        }

        /// The entries of the credentials that match `spec`, whose keys are the store's own: macOS takes
        /// `service` and `user` (exact, and every generic password of the login keychain without them), the
        /// Secret Service takes any attribute (`service`, `username`, ...), a mock store matches `service` and
        /// `user` as substrings. A store with no search (the Windows Credential Manager) is `unsupported`. On the
        /// Secret Service a search may ask the person to unlock.
        #[host("Wait")]
        pub async fn search(&self, spec: Option<HashMap<String, String>>) -> Result<Vec<Entry>, Error> {
            let store = self.inner.clone();
            blocking(move || {
                let spec = spec.unwrap_or_default();
                let spec: HashMap<&str, &str> = spec.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect();
                let entries = Credential::search_in_store(&store, &spec)?;
                Ok(entries.into_iter().map(|entry| Entry { inner: Arc::new(entry) }).collect())
            })
            .await
        }
    }

    #[mlua_ir::object]
    impl Entry {
        /// The credential's secret as text, or `(nil, nil)` when the store has no such credential. A secret that
        /// is not UTF-8 is `bad_encoding` (read it with `get_secret`). The platform may ask the person first, and
        /// the call waits for them.
        #[host("Wait")]
        pub async fn get_password(&self) -> Result<Option<String>, Error> {
            let entry = self.inner.clone();
            blocking(move || found(entry.get_password())).await
        }

        /// Store text as the credential's secret: the credential is made, or its secret replaced.
        #[host("Wait")]
        pub async fn set_password(&self, password: String) -> Result<(), Error> {
            let entry = self.inner.clone();
            blocking(move || Ok(entry.set_password(&password)?)).await
        }

        /// The credential's secret as the bytes the store holds (a string of bytes), or `(nil, nil)` when the
        /// store has no such credential.
        #[host("Wait")]
        pub async fn get_secret(&self) -> Result<Option<Bytes>, Error> {
            let entry = self.inner.clone();
            blocking(move || found(entry.get_secret().map(Bytes))).await
        }

        /// Store bytes (a string of bytes) as the credential's secret: the credential is made, or its secret
        /// replaced.
        #[host("Wait")]
        pub async fn set_secret(&self, secret: Bytes) -> Result<(), Error> {
            let entry = self.inner.clone();
            blocking(move || Ok(entry.set_secret(&secret.0)?)).await
        }

        /// Delete the credential from the store: `true`, or `false` when the store had no such credential.
        #[host("Wait")]
        pub async fn delete_credential(&self) -> Result<bool, Error> {
            let entry = self.inner.clone();
            blocking(move || match entry.delete_credential() {
                Ok(()) => Ok(true),
                Err(keyring_core::Error::NoEntry) => Ok(false),
                Err(e) => Err(e.into()),
            })
            .await
        }

        /// The service and the user this entry was made from, or `nil, nil` for an entry from `search` whose
        /// store does not say.
        pub fn get_specifiers(&self) -> (Option<String>, Option<String>) {
            self.inner.get_specifiers().unzip()
        }
    }
}

#[cfg(test)]
mod tests {
    //! Against `keyring-core`'s mock store: no test reaches the credential store of whoever runs them.
    //! `the_platform_store_keeps_a_credential` is the one test against a real store, and is ignored.

    use std::sync::Arc;

    use mlua::Value;

    use super::keyring::{Error, MOCK, chosen_store, new_mock};
    use crate::make_engine;

    /// Run `script` in a new engine.
    async fn run<R: mlua::FromLuaMulti>(script: &str) -> R {
        make_engine()
            .expect("engine")
            .load(script)
            .call_async(())
            .await
            .unwrap_or_else(|e| panic!("the script failed: {e}"))
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
        let (password, password_err, secret, secret_err, deleted, delete_err): (Value, Value, Value, Value, bool, Value) =
            run(r#"
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
        let (secret, kind, text): (mlua::LuaString, String, String) = lua
            .load(
                r#"
                local entry = assert(require("vnd:keyring").mock():entry("example.com", "key"))
                assert(entry:set_secret("\0\255binary\0") == true)
                local secret = assert(entry:get_secret())
                local password, err = entry:get_password()
                assert(password == nil)
                return secret, err.kind, tostring(err)
                "#,
            )
            .call_async(())
            .await
            .expect("the script");
        assert_eq!(&*secret.as_bytes(), b"\0\xffbinary\0");
        assert_eq!(kind, "bad_encoding");
        assert!(text.starts_with("vnd:keyring.Entry:get_password: bad_encoding: "), "{text}");
        assert!(!text.contains("binary"), "the secret is in the error");
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
        let (service, user): (String, String) =
            run(r#"return assert(require("vnd:keyring").mock():entry("example.com", "ada")):get_specifiers()"#).await;
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
        let (service, password, spec): (String, String, String) = run(r#"
            local keyring = require("vnd:keyring")
            local store = keyring.mock()
            local entry = assert(store:entry("example.com", "ada"))
            local ok_service, service = pcall(store.entry, store, "\255", "ada")
            local ok_password, password = pcall(entry.set_password, entry, "\255")
            local ok_spec, spec = pcall(store.search, store, { service = 1 })
            assert(not ok_service and not ok_password and not ok_spec)
            -- nothing was stored
            assert(entry:get_password() == nil)
            return tostring(service), tostring(password), tostring(spec)
            "#)
        .await;
        assert!(service.contains("vnd:keyring.Store:entry: argument #1 'service': expected UTF-8 text"), "{service}");
        assert!(
            password.contains("vnd:keyring.Entry:set_password: argument #1 'password': expected UTF-8 text"),
            "{password}"
        );
        assert!(spec.contains("vnd:keyring.Store:search: argument #1 'spec'"), "{spec}");
    }

    #[test]
    fn a_store_error_maps_to_its_kind() {
        use keyring_core::Error as Store;
        assert!(matches!(Error::from(Store::Invalid("service".into(), "is empty".into())), Error::InvalidInput { .. }));
        assert!(matches!(Error::from(Store::TooLong("user".into(), 255)), Error::InvalidInput { .. }));
        assert!(matches!(Error::from(Store::NoDefaultStore), Error::NoStore { .. }));
        assert!(matches!(Error::from(Store::NoStorageAccess("locked".into())), Error::NoAccess { .. }));
        assert!(matches!(Error::from(Store::NoEntry), Error::NotFound));
        assert!(matches!(Error::from(Store::Ambiguous(Vec::new())), Error::Ambiguous { matches: 0 }));
        assert!(matches!(Error::from(Store::BadStoreFormat("truncated".into())), Error::BadData { .. }));
        assert!(matches!(Error::from(Store::NotSupportedByStore("no search".into())), Error::Unsupported { .. }));
        assert!(matches!(Error::from(Store::PlatformFailure("-25300".into())), Error::Platform { .. }));
    }

    #[test]
    fn every_kind_is_snake_case_and_declared() {
        let declared = crate::declarations::luau();
        let errors = [
            Error::InvalidInput { reason: String::new() },
            Error::NoStore { reason: String::new() },
            Error::NoAccess { reason: String::new() },
            Error::NotFound,
            Error::Ambiguous { matches: 2 },
            Error::BadEncoding,
            Error::BadData { reason: String::new() },
            Error::Unsupported { reason: String::new() },
            Error::Platform { reason: String::new() },
            Error::Other { reason: String::new() },
        ];
        let kinds: Vec<String> = errors
            .iter()
            .map(|error| serde_json::to_value(error).unwrap()["kind"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(
            kinds,
            [
                "invalid_input",
                "no_store",
                "no_access",
                "not_found",
                "ambiguous",
                "bad_encoding",
                "bad_data",
                "unsupported",
                "platform",
                "other"
            ]
        );
        let line = declared.lines().find(|line| line.starts_with("export type vnd_keyring_ErrorKind =")).expect("the kinds");
        for kind in &kinds {
            assert!(line.contains(&format!("\"{kind}\"")), "`{kind}` is not declared: {line}");
        }
    }

    #[test]
    fn a_secret_that_came_with_an_error_is_not_in_the_error() {
        let secret = b"hunter2-not-utf8-\xff".to_vec();
        let not_text = Error::from(keyring_core::Error::BadEncoding(secret.clone()));
        assert!(matches!(not_text, Error::BadEncoding));
        assert!(!not_text.to_string().contains("hunter2"));
        assert!(!serde_json::to_string(&not_text).unwrap().contains("hunter2"));

        let malformed = Error::from(keyring_core::Error::BadDataFormat(secret, "bad padding".into()));
        assert!(matches!(malformed, Error::BadData { .. }));
        assert!(!malformed.to_string().contains("hunter2"));
        assert!(!serde_json::to_string(&malformed).unwrap().contains("hunter2"));
        assert!(malformed.to_string().contains("bad padding"));
    }

    #[test]
    fn the_environment_chooses_the_process_mock_and_refuses_anything_else() {
        let first = chosen_store(Some(MOCK)).expect("a mock store");
        let again = chosen_store(Some(MOCK)).expect("a mock store");
        assert!(Arc::ptr_eq(&first.inner, &again.inner), "one mock store for the process");
        assert_eq!(first.inner.vendor(), new_mock().expect("mock").inner.vendor());

        let refused = chosen_store(Some("memory")).err().expect("an error");
        assert!(matches!(refused, Error::InvalidInput { .. }));
        assert_eq!(refused.to_string(), "NEOC_KEYRING is `memory`: the one value it takes is `mock`");
    }

    /// The real store of this machine: `cargo test -p neoc-vm2 -- --ignored the_platform_store` (macOS may ask to
    /// allow the test binary; a Linux machine needs a Secret Service). It makes one credential and deletes it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "writes to the credential store of whoever runs it"]
    async fn the_platform_store_keeps_a_credential() {
        let (kept, deleted, after): (String, bool, Value) = run(r#"
            local store = assert(require("vnd:keyring").store())
            local entry = assert(store:entry("neoc-vm2.tests", "the_platform_store_keeps_a_credential"))
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
}
