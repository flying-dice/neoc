//! What the methods of a `vnd:bollard` client take that is this module's own: names that are checked before they
//! go into a request path, and each call's options (lower-case keys: `all`, `force`, `filters`, `tail`).
//!
//! Docker's own request bodies are not here: they are bollard's models, read through `mlua_ir::Foreign`.

use std::collections::HashMap;

use mlua::{Lua, Value};
use mlua_ir::ir::Ty;
use mlua_ir::{FromLuaArg, Options, Registry, Shape, Text};
use serde::{Deserialize, Serialize};

/// The longest name of a container, volume or network Docker takes, and far more than any image reference needs.
const MAX_NAME: usize = 512;

/// Docker's own alphabet for a name (`[A-Za-z0-9][A-Za-z0-9_.-]*`), so a name cannot reach another endpoint. An id
/// is hexadecimal and passes.
fn name(text: &str, what: &str) -> Result<(), String> {
    let first_ok = text.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric());
    let rest_ok = text.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'));
    if first_ok && rest_ok && text.len() <= MAX_NAME {
        Ok(())
    } else {
        Err(format!(
            "{what} must be a name or an id: letters, digits, `_`, `.` and `-`, starting with a letter or digit"
        ))
    }
}

fn container_name(text: &str) -> Result<(), String> {
    name(text, "the container")
}

fn volume_name(text: &str) -> Result<(), String> {
    name(text, "the volume")
}

fn network_name(text: &str) -> Result<(), String> {
    name(text, "the network")
}

/// A name's alphabet plus `/`, `:` and `@` (`registry:5000/team/app:1.2`, `app@sha256:...`), with no empty or
/// dot-only path part.
fn image_reference(text: &str) -> Result<(), String> {
    let chars_ok = text
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'/' | b':' | b'@'));
    let parts_ok = text.split('/').all(|part| !part.is_empty() && part != "." && part != "..");
    if chars_ok && parts_ok && text.len() <= MAX_NAME {
        Ok(())
    } else {
        Err("the image must be a reference like `nginx`, `nginx:1.27` or `registry:5000/team/app@sha256:...`"
            .to_owned())
    }
}

fn signal_name(text: &str) -> Result<(), String> {
    if !text.is_empty() && text.bytes().all(|b| b.is_ascii_alphanumeric()) {
        Ok(())
    } else {
        Err("a signal is its name or its number (`SIGTERM`, `HUP`, `9`)".to_owned())
    }
}

/// A checked text type that is also a field of an options table: serde reads it through the same check.
macro_rules! checked_text {
    ($name:ident, $check:ident) => {
        impl TryFrom<String> for $name {
            type Error = String;
            fn try_from(text: String) -> Result<Self, String> {
                $check(&text)?;
                Ok(Self(text))
            }
        }

        impl From<$name> for String {
            fn from(text: $name) -> String {
                text.0
            }
        }
    };
}

/// The name or the id of a container: letters, digits, `_`, `.` and `-`, starting with a letter or digit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Text)]
#[text(validate = "container_name")]
#[serde(try_from = "String", into = "String")]
#[shape(module = "vnd:bollard")]
pub struct ContainerName(pub String);
checked_text!(ContainerName, container_name);

/// The name of a volume: letters, digits, `_`, `.` and `-`, starting with a letter or digit.
#[derive(Debug, Clone, PartialEq, Eq, Text)]
#[text(validate = "volume_name")]
#[shape(module = "vnd:bollard")]
pub struct VolumeName(pub String);

/// The name or the id of a network: letters, digits, `_`, `.` and `-`, starting with a letter or digit.
#[derive(Debug, Clone, PartialEq, Eq, Text)]
#[text(validate = "network_name")]
#[shape(module = "vnd:bollard")]
pub struct NetworkName(pub String);

/// An image reference: `nginx`, `nginx:1.27`, `registry:5000/team/app@sha256:...`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Text)]
#[text(validate = "image_reference")]
#[serde(try_from = "String", into = "String")]
#[shape(module = "vnd:bollard")]
pub struct ImageRef(pub String);
checked_text!(ImageRef, image_reference);

impl ImageRef {
    /// The repository and the tag a pull names. Without a tag or a digest the tag is `latest`: Docker's API pulls
    /// *every* tag of a repository when none is given, which no caller of `pull_image` means.
    pub(super) fn pull_target(self) -> (String, Option<String>) {
        let reference = self.0;
        if reference.contains('@') {
            return (reference, None);
        }
        let last_part = reference.rsplit('/').next().unwrap_or(&reference);
        match last_part.rsplit_once(':') {
            Some((_, tag)) => {
                let repository = &reference[..reference.len() - tag.len() - 1];
                (repository.to_owned(), Some(tag.to_owned()))
            }
            None => (reference, Some("latest".to_owned())),
        }
    }
}

/// A signal as Docker takes it: its name (`SIGHUP`, `HUP`) or its number, as text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Text)]
#[text(validate = "signal_name")]
#[serde(try_from = "String", into = "String")]
#[shape(module = "vnd:bollard")]
pub struct Signal(pub String);
checked_text!(Signal, signal_name);

/// Bytes a call sends as they are: a string (any bytes), or a `buffer`. Declared `string`, the one bytes type the
/// declarations have.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Payload(pub Vec<u8>);

impl Shape for Payload {
    fn shape(_: &mut Registry) -> Ty {
        Ty::String
    }
}

impl FromLuaArg for Payload {
    fn from_lua_arg(value: Value, _: &Lua) -> Result<Self, String> {
        match value {
            Value::String(text) => Ok(Self(text.as_bytes().to_vec())),
            Value::Buffer(buffer) => Ok(Self(buffer.to_vec())),
            other => Err(mlua_ir::expected("a string or a buffer", &other)),
        }
    }
}

/// Docker's `filters`: each filter's name with the values it may have
/// (`{ label = { "app=web" }, status = { "running" } }`).
pub type Filters = HashMap<String, Vec<String>>;

/// A count or a Unix time as the `i32` Docker's query takes: what does not fit is the largest there is.
pub(super) fn clamped(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

/// The three files of a TLS client. All three or none, which is why they are one table.
#[derive(Debug, Clone, Serialize, Deserialize, Shape)]
#[serde(deny_unknown_fields)]
#[shape(module = "vnd:bollard")]
pub struct Tls {
    /// The path of the PEM file of the certificate authority the daemon's certificate is checked against.
    pub ca: String,
    /// The path of the PEM file of the client's certificate.
    pub cert: String,
    /// The path of the PEM file of the client's private key.
    pub key: String,
}

/// How to reach a daemon.
#[derive(Debug, Default, Serialize, Deserialize, Options)]
#[serde(default, deny_unknown_fields)]
#[shape(module = "vnd:bollard")]
pub struct ConnectOptions {
    /// `unix:///path/docker.sock` (or the bare path), `tcp://host:2375`, `http://host:2375` or
    /// `https://host:2376`. Without it `DOCKER_HOST` is used, and without that the local socket.
    pub host: Option<String>,
    /// How long a call waits for the daemon's answer, in seconds: more than zero. 120 when not given.
    pub timeout: Option<f64>,
    /// Connect over TLS with a client certificate.
    pub tls: Option<Tls>,
}

/// The options of `list_containers`.
#[derive(Debug, Default, Serialize, Deserialize, Options)]
#[serde(default, deny_unknown_fields)]
#[shape(module = "vnd:bollard")]
pub struct ListContainers {
    /// Stopped containers too; only running ones otherwise.
    pub all: bool,
    /// At most this many, the most recently created first.
    pub limit: Option<u32>,
    /// Add `SizeRw` and `SizeRootFs` to each summary.
    pub size: bool,
    /// Docker's filters: `{ label = { "com.docker.compose.project=blog" }, status = { "running" } }`.
    pub filters: Option<Filters>,
}

/// The options of `create_container` that are not part of Docker's body.
#[derive(Debug, Default, Serialize, Deserialize, Options)]
#[serde(default, deny_unknown_fields)]
#[shape(module = "vnd:bollard")]
pub struct CreateContainer {
    /// The container's name; Docker makes one up when not given.
    pub name: Option<ContainerName>,
    /// The platform to create it for (`linux/arm64`); the daemon's own when not given.
    pub platform: Option<String>,
}

/// The options of `stop_container` and `restart_container`.
#[derive(Debug, Default, Serialize, Deserialize, Options)]
#[serde(default, deny_unknown_fields)]
#[shape(module = "vnd:bollard")]
pub struct Stop {
    /// Seconds Docker waits after the signal before it kills the container (10 when not given); `-1` waits for
    /// ever.
    pub timeout: Option<i32>,
    /// The signal to send; the image's stop signal when not given, usually `SIGTERM`.
    pub signal: Option<Signal>,
}

/// The options of `kill_container`.
#[derive(Debug, Default, Serialize, Deserialize, Options)]
#[serde(default, deny_unknown_fields)]
#[shape(module = "vnd:bollard")]
pub struct Kill {
    /// The signal to send; `SIGKILL` when not given.
    pub signal: Option<Signal>,
}

/// The options of `remove_container`.
#[derive(Debug, Default, Serialize, Deserialize, Options)]
#[serde(default, deny_unknown_fields)]
#[shape(module = "vnd:bollard")]
pub struct RemoveContainer {
    /// Kill a running container first; a running one is `conflict` otherwise.
    pub force: bool,
    /// Remove the container's anonymous volumes with it.
    pub volumes: bool,
}

/// The options of `logs`.
#[derive(Debug, Serialize, Deserialize, Options)]
#[serde(default, deny_unknown_fields)]
#[shape(module = "vnd:bollard")]
pub struct Logs {
    /// Stay open for new output until the container stops; end after what there is otherwise.
    pub follow: bool,
    /// What the container wrote to its standard output.
    pub stdout: bool,
    /// What the container wrote to its standard error.
    pub stderr: bool,
    /// Start this many lines from the end; from the beginning when not given.
    pub tail: Option<u32>,
    /// Only output since this Unix time.
    pub since: Option<u32>,
    /// Only output until this Unix time.
    pub until: Option<u32>,
    /// Prefix each line with the time it was written.
    pub timestamps: bool,
}

impl Default for Logs {
    fn default() -> Self {
        Self {
            follow: false,
            stdout: true,
            stderr: true,
            tail: None,
            since: None,
            until: None,
            timestamps: false,
        }
    }
}

/// The options of `exec`.
#[derive(Debug, Default, Serialize, Deserialize, Options)]
#[serde(default, deny_unknown_fields)]
#[shape(module = "vnd:bollard")]
pub struct ExecOptions {
    /// Environment variables for the command, each `NAME=value`.
    pub env: Option<Vec<String>>,
    /// The user to run it as (`name`, `uid` or `uid:gid`); the container's own when not given.
    pub user: Option<String>,
    /// The directory to run it in; the container's own when not given.
    pub working_dir: Option<String>,
    /// Run it with extended privileges.
    pub privileged: Option<bool>,
}

/// The options of `list_images`.
#[derive(Debug, Default, Serialize, Deserialize, Options)]
#[serde(default, deny_unknown_fields)]
#[shape(module = "vnd:bollard")]
pub struct ListImages {
    /// Intermediate layers too.
    pub all: bool,
    /// Add each image's `RepoDigests`.
    pub digests: bool,
    /// Docker's filters: `{ reference = { "nginx:*" }, dangling = { "true" } }`.
    pub filters: Option<Filters>,
}

/// The options of `pull_image`.
#[derive(Debug, Default, Serialize, Deserialize, Options)]
#[serde(default, deny_unknown_fields)]
#[shape(module = "vnd:bollard")]
pub struct Pull {
    /// The platform to pull for (`linux/arm64`); the daemon's own when not given.
    pub platform: Option<String>,
}

/// The options of `remove_image`.
#[derive(Debug, Default, Serialize, Deserialize, Options)]
#[serde(default, deny_unknown_fields)]
#[shape(module = "vnd:bollard")]
pub struct RemoveImage {
    /// Remove it even when a container uses it; that is `conflict` otherwise.
    pub force: bool,
    /// Keep its untagged parents.
    pub noprune: bool,
}

/// The options of `remove_volume`.
#[derive(Debug, Default, Serialize, Deserialize, Options)]
#[serde(default, deny_unknown_fields)]
#[shape(module = "vnd:bollard")]
pub struct RemoveVolume {
    /// Remove it even when its driver refuses.
    pub force: bool,
}

/// The options of `list_volumes` and `list_networks`.
#[derive(Debug, Default, Serialize, Deserialize, Options)]
#[serde(default, deny_unknown_fields)]
#[shape(module = "vnd:bollard")]
pub struct Filtered {
    /// Docker's filters: `{ name = { "data" }, label = { "app=web" } }`.
    pub filters: Option<Filters>,
}

/// The options of `events`.
#[derive(Debug, Default, Serialize, Deserialize, Options)]
#[serde(default, deny_unknown_fields)]
#[shape(module = "vnd:bollard")]
pub struct Events {
    /// Replay what happened since this Unix time first.
    pub since: Option<u32>,
    /// End the stream at this Unix time; it stays open until it is closed otherwise.
    pub until: Option<u32>,
    /// Docker's filters: `{ type = { "container" }, event = { "start", "die" } }`.
    pub filters: Option<Filters>,
}

/// The options of `build_image`.
#[derive(Debug, Serialize, Deserialize, Options)]
#[serde(default, deny_unknown_fields)]
#[shape(module = "vnd:bollard")]
pub struct Build {
    /// The name to give the image, with its tag (`app:1`); the image is only known by its id when not given.
    pub t: Option<ImageRef>,
    /// Where the Dockerfile is in the archive: `Dockerfile` when not given.
    pub dockerfile: String,
    /// Labels to put on the image.
    pub labels: Option<HashMap<String, String>>,
    /// The values of the Dockerfile's `ARG`s, by name.
    pub buildargs: Option<HashMap<String, String>>,
    /// Remove the build's intermediate containers once it succeeds: `true` when not given, as for Docker.
    pub rm: bool,
    /// Remove them even when it fails.
    pub forcerm: bool,
    /// Use no cached layer.
    pub nocache: bool,
    /// Pull the `FROM` images again even when they are on the host.
    pub pull: bool,
}

impl Default for Build {
    fn default() -> Self {
        Self {
            t: None,
            dockerfile: "Dockerfile".to_owned(),
            labels: None,
            buildargs: None,
            rm: true,
            forcerm: false,
            nocache: false,
            pull: false,
        }
    }
}

/// The options of `attach_container`: which of the container's streams to attach to. Nothing when not given, as
/// for Docker.
#[derive(Debug, Default, Serialize, Deserialize, Options)]
#[serde(default, deny_unknown_fields)]
#[shape(module = "vnd:bollard")]
pub struct AttachContainer {
    /// Stay attached and read what the container writes from now on.
    pub stream: bool,
    /// Attach to its standard input, which `write` writes to.
    pub stdin: bool,
    /// Read its standard output.
    pub stdout: bool,
    /// Read its standard error.
    pub stderr: bool,
    /// Read what it wrote before, first.
    pub logs: bool,
    /// The keys that detach from the container when they are written to its input (`ctrl-p,ctrl-q`, Docker's
    /// own, when not given).
    pub detach_keys: Option<String>,
}

/// Where `upload_to_container` unpacks its archive.
#[derive(Debug, Clone, Serialize, Deserialize, Shape)]
#[serde(deny_unknown_fields)]
#[shape(module = "vnd:bollard")]
pub struct Upload {
    /// The directory in the container to unpack the archive into; it must exist.
    pub path: String,
    /// Refuse to replace a directory with a file, or a file with a directory.
    #[serde(default)]
    pub no_overwrite_dir_non_dir: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read<T: FromLuaArg>(lua: &Lua, source: &str) -> Result<T, String> {
        let value: Value = lua.load(source).eval().unwrap();
        T::from_lua_arg(value, lua)
    }

    #[test]
    fn a_name_is_dockers_alphabet_and_cannot_leave_its_path_segment() {
        for good in ["web", "web-1", "a.b_c", "0123abcdef", "A"] {
            assert_eq!(container_name(good), Ok(()), "{good:?}");
        }
        for bad in ["", "-web", ".", "..", "a/b", "a?b", "a b", "a%2fb", "a#b", "é"] {
            assert!(container_name(bad).is_err(), "{bad:?} was accepted");
            assert!(volume_name(bad).is_err() && network_name(bad).is_err(), "{bad:?} was accepted");
        }
        assert!(container_name(&"a".repeat(MAX_NAME + 1)).is_err());
    }

    #[test]
    fn an_image_reference_keeps_its_registry_tag_and_digest() {
        for good in
            ["nginx", "nginx:1.27", "library/nginx", "registry:5000/team/app:1.2", "app@sha256:abc123"]
        {
            assert_eq!(image_reference(good), Ok(()), "{good:?}");
        }
        for bad in ["", "/nginx", "nginx/", "a//b", "../etc", "a/./b", "a?b", "a b", "a%b"] {
            assert!(image_reference(bad).is_err(), "{bad:?} was accepted");
        }
    }

    #[test]
    fn a_pull_without_a_tag_names_latest_and_never_every_tag() {
        let target = |reference: &str| ImageRef(reference.to_owned()).pull_target();
        assert_eq!(target("nginx"), ("nginx".into(), Some("latest".into())));
        assert_eq!(target("nginx:1.27"), ("nginx".into(), Some("1.27".into())));
        assert_eq!(
            target("registry:5000/team/app"),
            ("registry:5000/team/app".into(), Some("latest".into()))
        );
        assert_eq!(
            target("registry:5000/team/app:1.2"),
            ("registry:5000/team/app".into(), Some("1.2".into()))
        );
        assert_eq!(target("app@sha256:abc"), ("app@sha256:abc".into(), None));
    }

    #[test]
    fn options_refuse_unknown_keys_and_wrong_types() {
        let lua = Lua::new();
        let defaults = read::<Logs>(&lua, "nil").unwrap();
        assert!(defaults.stdout && defaults.stderr && !defaults.follow && defaults.tail.is_none());
        let unknown = read::<Logs>(&lua, "{ tial = 5 }").unwrap_err();
        assert!(unknown.contains("unknown field `tial`"), "{unknown}");
        assert!(read::<Logs>(&lua, "'all'").is_err());

        let logs = read::<Logs>(&lua, "{ follow = true, tail = 50, stderr = false }").unwrap();
        assert_eq!((logs.follow, logs.tail, logs.stdout, logs.stderr), (true, Some(50), true, false));
        for refused in ["{ follow = 1 }", "{ tail = 1.5 }", "{ tail = 1e12 }", "{ tail = -1 }"] {
            assert!(read::<Logs>(&lua, refused).is_err(), "{refused} was accepted");
        }

        let connect = read::<ConnectOptions>(&lua, "{ timeout = 2.5 }").unwrap();
        assert_eq!(connect.timeout, Some(2.5));
        assert!(read::<ConnectOptions>(&lua, "{ host = true }").is_err());
        // the three files of a TLS client come together
        let tls =
            read::<ConnectOptions>(&lua, "{ tls = { ca = 'ca.pem', cert = 'cert.pem', key = 'key.pem' } }");
        assert_eq!(tls.unwrap().tls.unwrap().key, "key.pem");
        assert!(read::<ConnectOptions>(&lua, "{ tls = { ca = 'ca.pem' } }").is_err());
        assert!(read::<ConnectOptions>(&lua, "{ tls_ca = 'ca.pem' }").is_err());
    }

    #[test]
    fn filters_are_lists_of_values_by_name() {
        let lua = Lua::new();
        let listed =
            read::<ListImages>(&lua, "{ filters = { dangling = { 'true' }, label = { 'a=1', 'b' } } }")
                .unwrap();
        let filters = listed.filters.unwrap();
        assert_eq!(filters["dangling"], vec!["true"]);
        assert_eq!(filters["label"], vec!["a=1", "b"]);
        for refused in [
            "{ filters = { status = 1 } }",
            "{ filters = 'running' }",
            "{ filters = { status = 'running' } }",
            // a table of names is not a list of values: read as one it would be no filter at all
            "{ filters = { label = { ['app=web'] = true } } }",
        ] {
            assert!(read::<ListImages>(&lua, refused).is_err(), "{refused} was accepted");
        }
        assert!(read::<ExecOptions>(&lua, "{ env = { A = '1' } }").is_err());
        assert!(read::<ExecOptions>(&lua, "{ env = {} }").is_ok());
    }

    #[test]
    fn a_signal_is_a_name_or_a_number() {
        let lua = Lua::new();
        let kill = read::<Kill>(&lua, "{ signal = 'SIGHUP' }").unwrap();
        assert_eq!(kill.signal, Some(Signal("SIGHUP".to_owned())));
        let refused = read::<Kill>(&lua, "{ signal = 'HUP; rm' }").unwrap_err();
        assert!(refused.contains("a signal is its name or its number"), "{refused}");
        assert!(read::<Kill>(&lua, "{ signal = '' }").is_err());
        assert!(read::<Stop>(&lua, "{ timeout = 5, signal = '9' }").is_ok());
    }

    #[test]
    fn a_name_is_checked_as_an_argument_and_as_an_option() {
        let lua = Lua::new();
        assert_eq!(read::<ContainerName>(&lua, "'web-1'").unwrap().0, "web-1");
        let refused = read::<ContainerName>(&lua, "'a/b'").unwrap_err();
        assert!(refused.contains("the container must be a name or an id"), "{refused}");
        assert!(read::<ContainerName>(&lua, "42").is_err());
        assert!(read::<ImageRef>(&lua, "'nginx?tag=x'").is_err());
        let named = read::<CreateContainer>(&lua, "{ name = 'web' }").unwrap();
        assert_eq!(named.name, Some(ContainerName("web".to_owned())));
        let refused = read::<CreateContainer>(&lua, "{ name = 'a/b' }").unwrap_err();
        assert!(refused.contains("the container must be a name or an id"), "{refused}");
    }
}
