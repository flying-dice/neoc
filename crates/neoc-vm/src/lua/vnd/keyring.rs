//! The operating system's credential store — `vnd:keyring` (backed by the `keyring` crate and its `keyring-core`).
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
//! **Which store.** `store()` is the platform's own, chosen by the `keyring` crate: Keychain Services on macOS (the
//! login keychain), the Credential Manager on Windows, the Secret Service on Linux and the other Unixes (GNOME
//! Keyring, KWallet). A machine without one (a headless Linux box with no Secret Service) answers `no_store`.
//! `mock()` is `keyring-core`'s mock store: in memory, gone with the process, for tests. With `NEOC_KEYRING=mock`
//! in the environment `store()` is one mock store shared by the whole process, so a program can be run without it
//! touching the real store.
//!
//! **Entries.** A credential is named by a `service` and a `user`, both text. `Store:entry` makes an [`Entry`] for
//! that pair and touches nothing: the credential need not exist. Its secret is text (`get_password`,
//! `set_password`) or bytes (`get_secret`, `set_secret`). Reading a credential that is not there is `(nil, nil)`,
//! and deleting one is `false`: neither is an error.
//!
//! **Errors.** A call returns `(value, nil)` or `(nil, "keyring.<function>: <kind>: <message>")`; the kinds are
//! stable (`error`, `keyring.error_kind`; `keyring.error_message` is the message). No message holds a secret.
//!
//! **Waiting.** Every call that reaches the store runs on the blocking pool and the script waits for it: the
//! platform may ask the person (macOS shows a dialog before it hands a credential made by another program to this
//! one) and the call takes as long as they do.
//!
//! **What reaching the store means.** A script given this module reads and replaces any credential the platform
//! lets this process have, which on most machines is every credential of the user it runs as. Nothing here narrows
//! that, as nothing narrows `std:fs`: a script that is given this module is trusted with the user's secrets.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use keyring_core::{CredentialStore, Entry as Credential, Error};
use mlua::{FromLua, IntoLuaMulti, Lua, LuaString, Table, Value};
use tracing::Instrument;

use crate::lua::error_text;
use crate::lua::facade::args::{Argument, Refused};
use crate::lua::facade::{Bytes, DeclType, List, Map, Module};
use crate::lua::lifecycle::{Role, async_function};
use crate::lua::sandbox::{Fallible, from_err, ok};

mod error;

#[cfg(test)]
mod tests;

use error::{KeyringError, Kind};

/// The environment variable that makes `store()` a mock store ([`MOCK`] is its one value).
const CHOICE: &str = "NEOC_KEYRING";
const MOCK: &str = "mock";

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `vnd:keyring` surface through the facade. `store`, `mock`, `error_kind` and `error_message` are plain
/// functions; the `Store` and `Entry` types keep their own `impl UserData` and are recorded with
/// [`crate::lua::facade::Module::userdata`].
#[neoc_macros::lua_module]
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "vnd:keyring")?;

    /// The operating system's credential store: Keychain Services on macOS (the login keychain), the Credential Manager on Windows, the Secret Service on Linux and the other Unixes. A machine that has none, or whose store cannot be opened, is `no_store`. With `NEOC_KEYRING=mock` in the environment it is a mock store instead, one for the whole process (see `mock`). A script given this module is trusted with the secrets of the user it runs as. Errors are `keyring.<function>: <kind>: <message>` (see `error_kind`).
    #[lua]
    m.set(
        "store",
        async_function(lua, Role::Wait, |_, ()| {
            let op = "keyring.store";
            async move { reply(op, blocking(default_store).await) }.instrument(span(op))
        })?,
    )?;

    m.declared(
        mock_store::lua(),
        lua.create_function(|_, ()| mock_store().map_err(mlua::Error::external))?,
    )?;

    /// The stable kind inside an error string returned by a `vnd:keyring` function, or `nil` when the string is not one of theirs (or is `nil`, so `keyring.error_kind(err)` takes the `err` of any call). Such a string is `keyring.<function>: <kind>: <message>`. Kinds: `invalid_input` (an argument was refused, by this module or by the store), `no_store` (this machine has no credential store that can be used), `no_access` (the store is locked, or the person refused), `not_found` (the credential an entry from `search` stood for is gone), `ambiguous` (more than one credential matches the service and user), `bad_encoding` (the secret is not UTF-8 text: read it with `get_secret`), `bad_data` (what the store holds could not be read), `unsupported` (the store does not do this), `platform` (the platform's store failed), `other`. The message is the store's own where it answered; it is for a log and may change, and never holds a secret.
    #[lua(takes("err: string?"))]
    m.func("error_kind", error_text::reader(error::kind_of))?;
    /// The message inside an error string returned by a `vnd:keyring` function: everything after the kind in `keyring.<function>: <kind>: <message>`, or `nil` when the string is not one of theirs (or is `nil`, so `keyring.error_message(err)` takes the `err` of any call), the same rule as `error_kind`. It is the store's or this module's wording, and never holds a secret. It is for showing or logging as it is: branch on `error_kind`, not on these words, which differ between platforms and versions. Read it here instead of taking the error string apart.
    #[lua(takes("err: string?"))]
    m.func("error_message", error_text::reader(error::message_of))?;

    m.userdata("Store", |s| {
        Store::lua(s);
    });

    m.userdata("Entry", |e| {
        Entry::lua(e);
    });

    Ok(m)
}

fn span(op: &'static str) -> tracing::Span {
    tracing::info_span!(target: "lua::lib::keyring", "keyring.call", op)
}

// ---------------------------------------------------------------------------
// Stores
// ---------------------------------------------------------------------------

/// A credential store: the platform's, or a mock.
#[derive(Clone)]
struct Store {
    inner: Arc<CredentialStore>,
}

/// A credential's name in a store (or, from `search`, one credential of it).
#[derive(Clone)]
struct Entry {
    inner: Arc<Credential>,
}

/// The store `store()` hands out: the platform's, or the process's mock when the environment asks for one.
fn default_store() -> Result<Store, KeyringError> {
    let choice = std::env::var(CHOICE).ok();
    chosen_store(choice.as_deref())
}

/// [`default_store`] for a value of [`CHOICE`] (`None` when it is not set).
fn chosen_store(choice: Option<&str>) -> Result<Store, KeyringError> {
    match choice {
        None | Some("") => platform_store(),
        Some(MOCK) => process_mock(),
        Some(other) => Err(KeyringError::invalid_input(format!(
            "{CHOICE} is `{other}`: the one value it takes is `{MOCK}`"
        ))),
    }
}

/// The platform's store. The `keyring` crate opens it once per process and keeps the outcome, so a machine whose
/// store could not be opened (no Secret Service was running) says so until the process is restarted.
fn platform_store() -> Result<Store, KeyringError> {
    match ::keyring::Entry::store_status() {
        // the crate sets the store it opened as `keyring-core`'s default; nothing here sets another
        Ok(()) => keyring_core::get_default_store()
            .map(|inner| Store { inner })
            .ok_or_else(|| KeyringError::from(Error::NoDefaultStore)),
        Err(why) => Err(KeyringError::no_store(why)),
    }
}

/// The one mock store `NEOC_KEYRING=mock` gives every script of the process.
fn process_mock() -> Result<Store, KeyringError> {
    static SHARED: OnceLock<Store> = OnceLock::new();
    if let Some(store) = SHARED.get() {
        return Ok(store.clone());
    }
    let made = mock_store()?;
    Ok(SHARED.get_or_init(|| made).clone())
}

/// A new mock store, for tests: in memory, its own, and gone with the process. It takes any service and user, and `search` matches `service` and `user` as substrings.
#[neoc_macros::lua_fn(name = "mock", returns = "Store")]
fn mock_store() -> Result<Store, KeyringError> {
    let inner: Arc<CredentialStore> = keyring_core::mock::Store::new()?;
    Ok(Store { inner })
}

// ---------------------------------------------------------------------------
// Calls
// ---------------------------------------------------------------------------

/// What a finished call hands the script: its value, or `nil` and why not.
fn reply<R>(op: &str, result: Result<R, KeyringError>) -> mlua::Result<Fallible<R>> {
    match result {
        Ok(value) => ok(value),
        Err(e) => from_err(op, e),
    }
}

/// Runs `job` on the blocking pool and waits for it: a store call is the platform's, and may wait for the person.
async fn blocking<F, R>(job: F) -> Result<R, KeyringError>
where
    F: FnOnce() -> Result<R, KeyringError> + Send + 'static,
    R: Send + 'static,
{
    // Do nothing on the first poll. mlua polls a host call once before it tries to yield, and where Luau refuses
    // the yield (a `gsub` callback, a metamethod) it drops the call after that poll: a job started now would write
    // to the store although the script was told the call failed (`std:fs` has the same rule).
    tokio::task::yield_now().await;
    tokio::task::spawn_blocking(job).await.unwrap_or_else(|e| {
        Err(KeyringError::new(
            Kind::Other,
            format!("the credential store job failed: {e}"),
        ))
    })
}

/// An argument that is text, named `$what` when it is refused.
macro_rules! text_argument {
    ($(#[$doc:meta])* $name:ident, $what:literal) => {
        $(#[$doc])*
        struct $name(String);

        impl DeclType for $name {
            fn decl_type() -> String {
                String::decl_type()
            }
        }

        impl Argument for $name {
            fn read(value: Value, lua: &Lua) -> mlua::Result<Result<Self, Refused>> {
                let value = LuaString::from_lua(value, lua)?;
                Ok(text(&value, $what).map(Self))
            }
        }
    };
}

text_argument!(
    /// The service a credential is for.
    Service,
    "the service"
);
text_argument!(
    /// The user a credential is of.
    User,
    "the user"
);
text_argument!(
    /// A secret that is text.
    Password,
    "the password"
);

/// An argument that is text: a service, a user, a password.
fn text(value: &LuaString, what: &str) -> Result<String, Refused> {
    value
        .to_str()
        .map(|text| text.to_owned())
        .map_err(|_| Refused(format!("{what} is not UTF-8 text")))
}

/// A secret as the bytes a script gave: a string of bytes.
struct Secret(Vec<u8>);

impl DeclType for Secret {
    fn decl_type() -> String {
        Bytes::decl_type()
    }
}

impl Argument for Secret {
    fn read(value: Value, lua: &Lua) -> mlua::Result<Result<Self, Refused>> {
        Ok(Ok(Self(
            LuaString::from_lua(value, lua)?.as_bytes().to_vec(),
        )))
    }
}

/// The `spec` of a search: text under text.
struct Spec(HashMap<String, String>);

impl DeclType for Spec {
    fn decl_type() -> String {
        Map::<String, String>::decl_type()
    }
}

impl Argument for Spec {
    fn read(value: Value, lua: &Lua) -> mlua::Result<Result<Self, Refused>> {
        let spec = Table::from_lua(value, lua)?;
        let read = spec
            .pairs::<Value, Value>()
            .map(|pair| match pair {
                Ok((Value::String(key), Value::String(value))) => Ok((
                    text(&key, "a key of `spec`")?,
                    text(&value, "a value of `spec`")?,
                )),
                _ => Err(Refused("`spec` is a table of text under text".to_owned())),
            })
            .collect::<Result<_, Refused>>();
        Ok(read.map(Self))
    }
}

/// What `get_specifiers` hands a script: the service and the user, or `nil, nil`.
struct Specifiers(Option<(String, String)>);

impl DeclType for Specifiers {
    fn decl_type() -> String {
        "(string?, string?)".to_owned()
    }
}

impl IntoLuaMulti for Specifiers {
    fn into_lua_multi(self, lua: &Lua) -> mlua::Result<mlua::MultiValue> {
        let (service, user) = self.0.unzip();
        (service, user).into_lua_multi(lua)
    }
}

/// A read for which "the store has no such credential" is the answer `nil`.
fn found<R>(read: Result<R, Error>) -> Result<Option<R>, KeyringError> {
    match read {
        Ok(value) => Ok(Some(value)),
        Err(Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

#[neoc_macros::lua_methods("vnd:keyring", "Store")]
impl Store {
    /// The entry for a service and a user in this store. Nothing is read or written: the credential need not exist. What a store accepts as a service or a user is its own (`invalid_input`).
    async fn entry(&self, service: Service, user: User) -> Result<Entry, KeyringError> {
        let store = self.clone();
        blocking(move || {
            let entry = Credential::new_in_store(&store.inner, &service.0, &user.0)?;
            Ok(Entry {
                inner: Arc::new(entry),
            })
        })
        .await
    }

    /// The entries of the credentials that match `spec`, whose keys are the store's own: macOS takes `service` and `user` (exact, and every generic password of the login keychain without them), the Secret Service takes any attribute (`service`, `username`, ...), a mock store matches `service` and `user` as substrings. A store with no search (the Windows Credential Manager) is `unsupported`. On the Secret Service a search may ask the person to unlock.
    async fn search(&self, spec: Option<Spec>) -> Result<List<Entry>, KeyringError> {
        let store = self.clone();
        blocking(move || {
            let spec = spec.map_or_else(HashMap::new, |spec| spec.0);
            let spec: HashMap<&str, &str> = spec
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str()))
                .collect();
            let entries = Credential::search_in_store(&store.inner, &spec)?
                .into_iter()
                .map(|entry| Entry {
                    inner: Arc::new(entry),
                })
                .collect();
            Ok(List(entries))
        })
        .await
    }
}

#[neoc_macros::lua_methods("vnd:keyring", "Entry")]
impl Entry {
    /// The credential's secret as text, or `(nil, nil)` when the store has no such credential. A secret that is not UTF-8 is `bad_encoding` (read it with `get_secret`). The platform may ask the person first, and the call waits for them.
    async fn get_password(&self) -> Result<Option<String>, KeyringError> {
        let entry = self.clone();
        blocking(move || found(entry.inner.get_password())).await
    }

    /// Store text as the credential's secret: the credential is made, or its secret replaced.
    async fn set_password(&self, password: Password) -> Result<bool, KeyringError> {
        let entry = self.clone();
        blocking(move || {
            entry.inner.set_password(&password.0)?;
            Ok(true)
        })
        .await
    }

    /// The credential's secret as the bytes the store holds (a string of bytes), or `(nil, nil)` when the store has no such credential.
    async fn get_secret(&self) -> Result<Option<Bytes>, KeyringError> {
        let entry = self.clone();
        blocking(move || found(entry.inner.get_secret().map(Bytes))).await
    }

    /// Store bytes (a string of bytes) as the credential's secret: the credential is made, or its secret replaced.
    async fn set_secret(&self, secret: Secret) -> Result<bool, KeyringError> {
        let entry = self.clone();
        blocking(move || {
            entry.inner.set_secret(&secret.0)?;
            Ok(true)
        })
        .await
    }

    /// Delete the credential from the store: `true`, or `false` when the store had no such credential.
    async fn delete_credential(&self) -> Result<bool, KeyringError> {
        let entry = self.clone();
        blocking(move || match entry.inner.delete_credential() {
            Ok(()) => Ok(true),
            Err(Error::NoEntry) => Ok(false),
            Err(e) => Err(e.into()),
        })
        .await
    }

    /// The service and the user this entry was made from, or `nil, nil` for an entry from `search` whose store does not say.
    fn get_specifiers(&self) -> Specifiers {
        Specifiers(self.inner.get_specifiers())
    }
}
