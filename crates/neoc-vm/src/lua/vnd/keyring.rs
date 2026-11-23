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
use mlua::{Lua, LuaString, Table, UserData, UserDataMethods, Value};
use tracing::Instrument;

use crate::lua::error_text;
use crate::lua::facade::Module;
use crate::lua::lifecycle::{Role, async_function, async_method};
use crate::lua::sandbox::{from_err, ok};

mod error;

#[cfg(test)]
mod tests;

use error::{KeyringError, Kind};

/// The environment variable that makes `store()` a mock store ([`MOCK`] is its one value).
const CHOICE: &str = "NEOC_KEYRING";
const MOCK: &str = "mock";

const ENTRY: &str = "Result<Entry, string>";
const DONE: &str = "Result<bool, string>";

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `vnd:keyring` surface through the facade. `store`, `mock`, `error_kind` and `error_message` are plain
/// functions; the `Store` and `Entry` types keep their own `impl UserData` and are recorded with
/// [`crate::lua::facade::Module::userdata`].
#[neoc_macros::lua_module]
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "vnd:keyring")?;

    let store = async_function(lua, Role::Wait, |lua, ()| {
        let op = "keyring.store";
        async move {
            let opened = blocking(|| default_store().map(Reply::Store)).await;
            reply(&lua, op, opened)
        }
        .instrument(span(op))
    })?;
    /// The operating system's credential store: Keychain Services on macOS (the login keychain), the Credential Manager on Windows, the Secret Service on Linux and the other Unixes. A machine that has none, or whose store cannot be opened, is `no_store`. With `NEOC_KEYRING=mock` in the environment it is a mock store instead, one for the whole process (see `mock`). A script given this module is trusted with the secrets of the user it runs as. Errors are `keyring.<function>: <kind>: <message>` (see `error_kind`).
    #[lua("Result<Store, string>", takes())]
    m.set("store", store)?;

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
        s.method(
            "entry",
            &[("service", "string"), ("user", "string")],
            ENTRY,
            "The entry for a service and a user in this store. Nothing is read or written: the credential need not exist. What a store accepts as a service or a user is its own (`invalid_input`).",
        )
        .method(
            "search",
            &[("spec", "{ [string]: string }?")],
            "Result<{ Entry }, string>",
            "The entries of the credentials that match `spec`, whose keys are the store's own: macOS takes `service` and `user` (exact, and every generic password of the login keychain without them), the Secret Service takes any attribute (`service`, `username`, ...), a mock store matches `service` and `user` as substrings. A store with no search (the Windows Credential Manager) is `unsupported`. On the Secret Service a search may ask the person to unlock.",
        );
    });

    m.userdata("Entry", |e| {
        e.method(
            "get_password",
            &[],
            "Result<string, string>",
            "The credential's secret as text, or `(nil, nil)` when the store has no such credential. A secret that is not UTF-8 is `bad_encoding` (read it with `get_secret`). The platform may ask the person first, and the call waits for them.",
        )
        .method(
            "set_password",
            &[("password", "string")],
            DONE,
            "Store text as the credential's secret: the credential is made, or its secret replaced.",
        )
        .method(
            "get_secret",
            &[],
            "Result<string, string>",
            "The credential's secret as the bytes the store holds (a string of bytes), or `(nil, nil)` when the store has no such credential.",
        )
        .method(
            "set_secret",
            &[("secret", "string")],
            DONE,
            "Store bytes (a string of bytes) as the credential's secret: the credential is made, or its secret replaced.",
        )
        .method(
            "delete_credential",
            &[],
            DONE,
            "Delete the credential from the store: `true`, or `false` when the store had no such credential.",
        )
        .method(
            "get_specifiers",
            &[],
            "(string?, string?)",
            "The service and the user this entry was made from, or `nil, nil` for an entry from `search` whose store does not say.",
        );
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

/// What a finished call hands the script.
enum Reply {
    /// It was done (`true`), or there was nothing to do it to (`false`: `delete_credential`).
    Done(bool),
    /// There is no such credential, and that is an answer: `nil`.
    Missing,
    Password(String),
    Secret(Vec<u8>),
    Store(Store),
    Entry(Entry),
    Entries(Vec<Entry>),
}

fn reply(lua: &Lua, op: &str, result: Result<Reply, KeyringError>) -> mlua::Result<(Value, Value)> {
    match result {
        Ok(Reply::Done(done)) => ok(lua, done),
        Ok(Reply::Missing) => Ok((Value::Nil, Value::Nil)),
        Ok(Reply::Password(password)) => ok(lua, password),
        Ok(Reply::Secret(secret)) => ok(lua, lua.create_string(&secret)?),
        Ok(Reply::Store(store)) => ok(lua, store),
        Ok(Reply::Entry(entry)) => ok(lua, entry),
        Ok(Reply::Entries(entries)) => ok(lua, entries),
        Err(e) => from_err(lua, op, e),
    }
}

/// Runs `job` on the blocking pool and waits for it: a store call is the platform's, and may wait for the person.
async fn blocking<F>(job: F) -> Result<Reply, KeyringError>
where
    F: FnOnce() -> Result<Reply, KeyringError> + Send + 'static,
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

/// Register the method `op` (`keyring.<method>`) of a store or an entry. `prepare` reads the arguments in the
/// synchronous part of the call; `run` is the store call, made on the blocking pool.
fn call<T, M, A, P>(
    methods: &mut M,
    op: &'static str,
    prepare: impl Fn(A) -> Result<P, KeyringError> + Send + 'static,
    run: impl Fn(&T, P) -> Result<Reply, KeyringError> + Send + Sync + 'static,
) where
    T: Clone + Send + Sync + 'static,
    M: UserDataMethods<T>,
    A: mlua::FromLuaMulti,
    P: Send + 'static,
{
    let run = Arc::new(run);
    let name = op.strip_prefix("keyring.").unwrap_or(op);
    async_method(methods, name, Role::Wait, move |lua, this, args: A| {
        let this: T = this.clone();
        let prepared = prepare(args);
        let run = Arc::clone(&run);
        async move {
            let result = match prepared {
                Ok(prepared) => blocking(move || run(&this, prepared)).await,
                Err(e) => Err(e),
            };
            reply(&lua, op, result)
        }
        .instrument(span(op))
    });
}

/// A call with no arguments.
#[allow(clippy::unnecessary_wraps)] // the shape `call` takes
fn nothing((): ()) -> Result<(), KeyringError> {
    Ok(())
}

/// An argument that is text: a service, a user, a password.
fn text(value: &LuaString, what: &str) -> Result<String, KeyringError> {
    value
        .to_str()
        .map(|text| text.to_owned())
        .map_err(|_| KeyringError::invalid_input(format!("{what} is not UTF-8 text")))
}

/// The `spec` of a search: text under text.
fn spec_of(spec: Option<Table>) -> Result<HashMap<String, String>, KeyringError> {
    let Some(spec) = spec else {
        return Ok(HashMap::new());
    };
    let refused = || KeyringError::invalid_input("`spec` is a table of text under text");
    spec.pairs::<Value, Value>()
        .map(|pair| match pair {
            Ok((Value::String(key), Value::String(value))) => Ok((
                text(&key, "a key of `spec`")?,
                text(&value, "a value of `spec`")?,
            )),
            _ => Err(refused()),
        })
        .collect()
}

/// A read for which "the store has no such credential" is the answer `nil`.
fn found(read: Result<Reply, Error>) -> Result<Reply, KeyringError> {
    match read {
        Err(Error::NoEntry) => Ok(Reply::Missing),
        other => other.map_err(KeyringError::from),
    }
}

impl UserData for Store {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        call(
            methods,
            "keyring.entry",
            |(service, user): (LuaString, LuaString)| {
                Ok((text(&service, "the service")?, text(&user, "the user")?))
            },
            |store: &Store, (service, user)| {
                let entry = Credential::new_in_store(&store.inner, &service, &user)?;
                Ok(Reply::Entry(Entry {
                    inner: Arc::new(entry),
                }))
            },
        );
        call(methods, "keyring.search", spec_of, |store: &Store, spec| {
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
            Ok(Reply::Entries(entries))
        });
    }
}

impl UserData for Entry {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        call(
            methods,
            "keyring.get_password",
            nothing,
            |entry: &Entry, ()| found(entry.inner.get_password().map(Reply::Password)),
        );
        call(
            methods,
            "keyring.set_password",
            |password: LuaString| text(&password, "the password"),
            |entry: &Entry, password| {
                entry.inner.set_password(&password)?;
                Ok(Reply::Done(true))
            },
        );
        call(
            methods,
            "keyring.get_secret",
            nothing,
            |entry: &Entry, ()| found(entry.inner.get_secret().map(Reply::Secret)),
        );
        call(
            methods,
            "keyring.set_secret",
            |secret: LuaString| Ok(secret.as_bytes().to_vec()),
            |entry: &Entry, secret| {
                entry.inner.set_secret(&secret)?;
                Ok(Reply::Done(true))
            },
        );
        call(
            methods,
            "keyring.delete_credential",
            nothing,
            |entry: &Entry, ()| match entry.inner.delete_credential() {
                Ok(()) => Ok(Reply::Done(true)),
                Err(Error::NoEntry) => Ok(Reply::Done(false)),
                Err(e) => Err(e.into()),
            },
        );
        methods.add_method("get_specifiers", |_, entry, ()| {
            Ok(match entry.inner.get_specifiers() {
                Some((service, user)) => (Some(service), Some(user)),
                None => (None, None),
            })
        });
    }
}
