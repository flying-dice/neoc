//! The AG-UI protocol, client side: `lib:agui`, over the `neoc-agui` crate.
//!
//! AG-UI (<https://docs.ag-ui.com>) is how an agent and the program in front of it talk: the program POSTs a
//! `RunAgentInput` (the conversation, the tools it offers, the state) and the agent answers with a stream of events
//! (`text/event-stream`), one JSON object each. The tables a script reads and writes have the fields of the official
//! Python SDK's `ag_ui.core` under their Python names (`thread_id`, `message_id`, `tool_call_id`); the wire is
//! camelCase, and the module translates.
//!
//! ```lua
//! local agui = require("lib:agui")
//! local run, err = agui.run({
//!     url = "http://127.0.0.1:8000/agent",
//!     input = { thread_id = "t1", run_id = "r1", messages = { { id = "m1", role = "user", content = "hello" } } },
//! })
//! if not run then error(err) end
//! while true do
//!     local event, err = run:next()
//!     if err then error(err) end
//!     if not event then break end          -- the stream ended
//!     if event.type == "TEXT_MESSAGE_CONTENT" then io.write(event.delta) end
//! end
//! ```
//!
//! The request is `vnd:hyper`'s (`client`): the same options, connection, TLS and errors, read as a stream instead
//! of a whole body. What differs when the options say nothing:
//!
//! - **Redirects are not followed.** The request carries the conversation and often a credential; where it goes is
//!   the URL that was given. A 3xx is a `status` error.
//! - **It waits longer for data**: `idle_timeout` is 300 s, not 60. An agent that is thinking sends nothing.
//! - **One event is bounded** (`max_event`, 8 MiB), not the stream: a run is as long as it is. `timeout`, when
//!   given, bounds the run as a whole.
//!
//! A failure is `(nil, "agui.<fn>: <kind>: <message>")`; `agui.error_kind(err)` gives the kind: one of
//! `vnd:hyper`'s, `status` (the endpoint answered with something other than 2xx) or `protocol` (the answer is not an
//! event stream, or an event in it is not an AG-UI event).

use std::sync::Arc;

use bytes::Bytes;
use futures::StreamExt;
use mlua::serde::ser::Options as SerializeOptions;
use mlua::{Lua, LuaSerdeExt, Table, UserData, UserDataMethods, Value};
use neoc_agui::{RunAgentInput, StreamError};
use tokio::sync::{Mutex, mpsc};
use tokio::task::AbortHandle;

use crate::lua::error_text;
use crate::lua::facade::Module;
use crate::lua::lifecycle::{Life, Role, async_function, async_method};
use crate::lua::sandbox::{err, ok};
use crate::lua::vnd::hyper::client::{HyperError, Kind, Prepared, Streamed};

/// How long a run may send nothing before it is given up, unless `idle_timeout` says otherwise.
const DEFAULT_IDLE_SECS: f64 = 300.0;
/// How many events wait between the agent and the script before the reading task waits too.
const QUEUE: usize = 64;
/// How much of an error response's body is quoted in the error.
const ERROR_BODY: usize = 2048;
/// The options of `run` that are `vnd:hyper`'s, passed to it as they are.
const HYPER_OPTIONS: &[&str] = &["ca_pem", "connect_timeout", "idle_timeout", "timeout"];

/// A failure of this module: a kind (one of `vnd:hyper`'s, `status` or `protocol`) and a message.
struct AguiError {
    kind: &'static str,
    message: String,
}

impl AguiError {
    fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    fn invalid_input(message: impl Into<String>) -> Self {
        Self::new(Kind::InvalidInput.as_str(), message)
    }

    fn text(&self, function: &str) -> String {
        format!("agui.{function}: {}: {}", self.kind, self.message)
    }
}

impl From<HyperError> for AguiError {
    fn from(e: HyperError) -> Self {
        // a `HyperError` displays as `<kind>: <message>`
        let text = e.to_string();
        let (kind, message) = text.split_once(": ").unwrap_or(("other", &text));
        Self::new(Kind::parse(kind).map_or("other", Kind::as_str), message)
    }
}

impl From<StreamError<HyperError>> for AguiError {
    fn from(e: StreamError<HyperError>) -> Self {
        match e {
            StreamError::Transport(e) => e.into(),
            StreamError::TooLarge(_) => Self::new(Kind::TooLarge.as_str(), e.to_string()),
            StreamError::Framing(_) | StreamError::Event(_) => Self::new("protocol", e.to_string()),
        }
    }
}

/// The kind and the message of one of this module's error strings; the shape is [`error_text`]'s.
fn read(err: &str) -> Option<(&'static str, &str)> {
    error_text::read("agui", err, |name| match name {
        "status" => Some("status"),
        "protocol" => Some("protocol"),
        other => Kind::parse(other).map(Kind::as_str),
    })
}

/// The kind inside an error string of this module, or `None` when the text is not one of ours.
fn kind_of(err: &str) -> Option<&'static str> {
    read(err).map(|(kind, _)| kind)
}

/// The message inside an error string of this module: everything after the kind.
fn message_of(err: &str) -> Option<&str> {
    read(err).map(|(_, message)| message)
}

/// A value of the crate as a plain Lua value with the Python SDK's field names; a JSON `null` is `nil`.
fn to_lua<T: serde::Serialize>(lua: &Lua, value: &T) -> mlua::Result<Value> {
    let nil_for_null = SerializeOptions::new()
        .serialize_none_to_null(false)
        .serialize_unit_to_null(false);
    neoc_agui::snake::serialize(
        value,
        mlua::serde::Serializer::new_with_options(lua, nil_for_null),
    )
}

/// What `run` was asked for, read from its options: the request, ready to send, and the event limit.
struct Request {
    prepared: Prepared,
    max_event: usize,
}

impl Request {
    /// Reads the options. Runs in the synchronous part of the call (a table can run script code when it is read;
    /// see `vnd:hyper`'s `client`).
    fn read(lua: &Lua, opts: &Table) -> Result<Self, AguiError> {
        let lua_error = |e: mlua::Error| AguiError::invalid_input(e.to_string());
        let mut url = None;
        let mut input = None;
        let mut headers = None;
        let mut max_event = neoc_agui::DEFAULT_MAX_EVENT;
        let request = lua.create_table().map_err(lua_error)?;
        for pair in opts.pairs::<Value, Value>() {
            let (key, value) = pair.map_err(lua_error)?;
            let name = match &key {
                Value::String(s) => s.to_string_lossy(),
                other => {
                    return Err(AguiError::invalid_input(format!(
                        "an option named by a {}",
                        other.type_name()
                    )));
                }
            };
            match name.as_str() {
                "url" => url = Some(value),
                "input" => input = Some(value),
                "headers" => headers = Some(value),
                "max_event" => {
                    max_event = match value {
                        Value::Integer(n) if n > 0 => usize::try_from(n).unwrap_or(usize::MAX),
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        // checked: whole, positive
                        Value::Number(n) if n >= 1.0 && n.fract() == 0.0 && n <= 9.0e15 => {
                            n as usize
                        }
                        _ => {
                            return Err(AguiError::invalid_input(
                                "max_event must be a positive whole number of bytes",
                            ));
                        }
                    };
                }
                known if HYPER_OPTIONS.contains(&known) => {
                    request.set(key, value).map_err(lua_error)?
                }
                unknown => {
                    return Err(AguiError::invalid_input(format!(
                        "unknown option `{unknown}`"
                    )));
                }
            }
        }
        let input = match input {
            Some(input @ Value::Table(_)) => lua
                .from_value::<RunAgentInput>(input)
                .map_err(|e| AguiError::invalid_input(format!("input: {e}")))?,
            _ => {
                return Err(AguiError::invalid_input(
                    "input must be a table: the RunAgentInput",
                ));
            }
        };
        let body = serde_json::to_vec(&input)
            .map_err(|e| AguiError::invalid_input(format!("input: {e}")))?;

        // ours unless the caller set them (a name given twice in any case is refused by `vnd:hyper`)
        let sent = lua.create_table().map_err(lua_error)?;
        let mut has = (false, false);
        match headers {
            None | Some(Value::Nil) => {}
            Some(Value::Table(given)) => {
                for pair in given.pairs::<Value, Value>() {
                    let (name, value) = pair.map_err(lua_error)?;
                    if let Value::String(name) = &name {
                        let lower = name.to_string_lossy().to_ascii_lowercase();
                        has.0 |= lower == "content-type";
                        has.1 |= lower == "accept";
                    }
                    sent.set(name, value).map_err(lua_error)?;
                }
            }
            Some(_) => return Err(AguiError::invalid_input("headers must be a table")),
        }
        if !has.0 {
            sent.set("content-type", "application/json")
                .map_err(lua_error)?;
        }
        if !has.1 {
            sent.set("accept", "text/event-stream").map_err(lua_error)?;
        }

        (|| {
            request.set("url", url.unwrap_or(Value::Nil))?;
            request.set("headers", sent)?;
            request.set("body", lua.create_string(&body)?)?;
            request.set("follow_redirects", false)?;
            if !request.contains_key("idle_timeout")? {
                request.set("idle_timeout", DEFAULT_IDLE_SECS)?;
            }
            Ok(())
        })()
        .map_err(lua_error)?;
        Ok(Self {
            prepared: Prepared::post(&request)?,
            max_event,
        })
    }

    /// Sends the request and starts reading the answer, which must be an event stream.
    async fn send(self, life: &Life) -> Result<Run, AguiError> {
        let mut streamed = Streamed::open(self.prepared, life.clone()).await?;
        if !(200..300).contains(&streamed.status) {
            let said = quote(&mut streamed).await;
            return Err(AguiError::new(
                "status",
                format!("the endpoint answered {}{said}", streamed.status),
            ));
        }
        let content_type = streamed
            .headers
            .iter()
            .find_map(|(name, value)| (name == "content-type").then_some(value.as_str()))
            .unwrap_or("");
        let media = content_type.split(';').next().unwrap_or("").trim();
        if !media.eq_ignore_ascii_case("text/event-stream") {
            return Err(AguiError::new(
                "protocol",
                format!("the endpoint answered with `{content_type}`, not text/event-stream"),
            ));
        }

        let max_event = self.max_event;
        let (send, queue) = mpsc::channel(QUEUE);
        let reader = life.spawn(async move {
            let bytes = futures::stream::unfold(streamed, |mut streamed| async move {
                streamed
                    .chunk()
                    .await
                    .transpose()
                    .map(|chunk: Result<Bytes, HyperError>| (chunk, streamed))
            });
            let mut events = std::pin::pin!(neoc_agui::event_stream(bytes, max_event));
            while let Some(event) = events.next().await {
                // the stream ends at its first error: `event_stream` yields nothing after one
                if send.send(event.map_err(AguiError::from)).await.is_err() {
                    return;
                }
            }
        });
        Ok(Run {
            inner: Arc::new(Inner {
                queue: Mutex::new(queue),
                reader: reader.abort_handle(),
                closed: std::sync::atomic::AtomicBool::new(false),
            }),
        })
    }
}

/// The start of an error response's body, for the error message: `: <text>`, or nothing.
async fn quote(streamed: &mut Streamed) -> String {
    let mut body = Vec::new();
    while body.len() < ERROR_BODY {
        match streamed.chunk().await {
            Ok(Some(chunk)) => body.extend_from_slice(&chunk),
            _ => break,
        }
    }
    let text = String::from_utf8_lossy(&body);
    let text: String = text.chars().take(ERROR_BODY).collect();
    let text = text.trim();
    if text.is_empty() {
        String::new()
    } else {
        format!(": {text}")
    }
}

/// A run in progress: a task that reads the agent's answer, and a bounded queue between it and the script, so a
/// script that reads slowly slows the reading down. The task belongs to the engine's [`Life`] and stops with it,
/// when the run is closed, and when it is dropped; stopping it closes the connection.
pub struct Run {
    inner: Arc<Inner>,
}

struct Inner {
    queue: Mutex<mpsc::Receiver<Result<neoc_agui::Event, AguiError>>>,
    reader: AbortHandle,
    /// The script closed the run: the events already read are not given out any more.
    closed: std::sync::atomic::AtomicBool,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

impl UserData for Run {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        async_method(methods, "next", Role::Wait, |lua, this, (): ()| {
            let inner = Arc::clone(&this.inner);
            async move {
                let closed = || inner.closed.load(std::sync::atomic::Ordering::SeqCst);
                if closed() {
                    return Ok((Value::Nil, Value::Nil));
                }
                // one reader at a time: a second `next` from another task waits for the first
                let item = inner.queue.lock().await.recv().await;
                match item {
                    // closed while this waited, or with events still queued: the run is over for the script
                    _ if closed() => Ok((Value::Nil, Value::Nil)),
                    Some(Ok(event)) => ok(&lua, to_lua(&lua, &event)?),
                    Some(Err(e)) => err(&lua, e.text("next")),
                    None => Ok((Value::Nil, Value::Nil)),
                }
            }
        });
        methods.add_method("close", |_, this, (): ()| {
            this.inner
                .closed
                .store(true, std::sync::atomic::Ordering::SeqCst);
            this.inner.reader.abort();
            Ok(())
        });
    }
}

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

const JSON: &str = "any";

/// Declares the `lib:agui` surface through the facade. `run` is set verbatim (it reads its options before it builds
/// its future, and bakes its own error prefix), as `vnd:hyper`'s client functions are.
#[allow(clippy::too_many_lines)] // one declaration after another
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:agui")?;

    m.set(
        "run",
        async_function(lua, Role::Wait, |lua, opts: Table| {
            let request = Request::read(&lua, &opts);
            let life = Life::of(&lua);
            async move {
                let started = match request {
                    Ok(request) => request.send(&life).await,
                    Err(e) => Err(e),
                };
                match started {
                    Ok(run) => ok(&lua, run),
                    Err(e) => err(&lua, e.text("run")),
                }
            }
        })?,
    )?;
    m.record_fn(
        "run",
        &[(
            "opts",
            "{ url: string, input: RunAgentInput, headers: { [string]: string }?, ca_pem: string?, connect_timeout: number?, idle_timeout: number?, timeout: number?, max_event: number? }",
        )],
        "Result<Run, string>",
        "Start a run: POST `input` to the agent's AG-UI endpoint at `url` (`http` or `https`) and read its answer as a stream of events. Returns when the endpoint has answered: a `Run` whose `next()` gives the events, or `(nil, err)` with `err` = `agui.run: <kind>: <message>` (see `error_kind`). `input` is the `RunAgentInput`: `thread_id` and `run_id` are required, and `messages`, `tools`, `context`, `state`, `forwarded_props`, `parent_run_id` and `resume` are sent when given; a field the protocol does not have is ignored, and one of the wrong type is `invalid_input`. `headers` are added to the request (`Authorization` for an endpoint that wants a token); `Content-Type: application/json` and `Accept: text/event-stream` are sent unless given. `ca_pem`, `connect_timeout` (default 10) and `timeout` (the whole run; default none) are `vnd:hyper`'s. `idle_timeout` (seconds without data; default 300, since an agent that is thinking sends nothing). `max_event` (bytes, default 8388608) bounds one event, not the run. Redirects are not followed: a 3xx, like any answer that is not 2xx, is a `status` error that quotes the start of the body. An answer that is not `text/event-stream` is `protocol`. The connection belongs to the script: it closes when the run is closed or collected and when the script stops.",
    );

    m.set(
        "apply_patch",
        lua.create_function(
            |lua, (document, patch): (Value, Value)| -> mlua::Result<(Value, Value)> {
                let invalid = |message: String| {
                    err(lua, AguiError::invalid_input(message).text("apply_patch"))
                };
                let mut document: serde_json::Value = match lua.from_value(document) {
                    Ok(document) => document,
                    Err(e) => return invalid(format!("document: {e}")),
                };
                let patch: Vec<serde_json::Value> = match patch {
                    Value::Table(ref t) if t.is_empty() => Vec::new(),
                    patch => match lua.from_value(patch) {
                        Ok(patch) => patch,
                        Err(e) => return invalid(format!("patch: {e}")),
                    },
                };
                match neoc_agui::apply_patch(&mut document, &patch) {
                    Ok(()) => ok(lua, to_lua(lua, &document)?),
                    Err(e) => invalid(e.to_string()),
                }
            },
        )?,
    )?;
    m.record_fn(
        "apply_patch",
        &[("document", JSON), ("patch", "{ any }")],
        "(any, string?)",
        "Apply a JSON Patch (RFC 6902: what `STATE_DELTA.delta` and `ACTIVITY_DELTA.patch` are) to `document` and return the new document; `document` itself is not changed. All of the patch applies or none of it: `(nil, err)` with kind `invalid_input` when `patch` is not a list of operations or an operation cannot be applied (a path that is not there, a failed `test`). A table with no entries is an empty object; JSON `null` is `nil`, so a patch cannot store one in an array.",
    );

    m.func(
        "error_kind",
        &[("err", "string?")],
        "The stable kind inside an error string this module returned, or `nil` when the string is not one of its own (or is `nil`, so `agui.error_kind(err)` takes the `err` of any call). Such a string is `agui.<function>: <kind>: <message>`. Kinds: those of `vnd:hyper` (`invalid_input`, `invalid_url`, `connect`, `tls`, `timed_out`, `too_large` for an event over `max_event`, `connection_lost`, `bad_response`, `other`), `status` (the endpoint answered, and not with 2xx) and `protocol` (the answer is not an event stream, or an event in it is not an AG-UI event: a field missing or of the wrong type). The message is for a log and may change.",
        error_text::reader(kind_of),
    )?;
    m.func(
        "error_message",
        &[("err", "string?")],
        "The message inside an error string this module returned: everything after the kind in `agui.<function>: <kind>: <message>`, or `nil` when the string is not one of its own (or is `nil`). It is for a person or a log, and its wording may change.",
        error_text::reader(message_of),
    )?;

    let types = lua.create_table()?;
    for name in neoc_agui::events::EVENT_TYPES {
        types.raw_push(neoc_agui::events::screaming(name))?;
    }
    types.set_readonly(true);
    m.set("EVENT_TYPES", types)?;
    m.const_(
        "EVENT_TYPES",
        "{ string }",
        "Every event type the module reads, in the Python SDK's order (`TEXT_MESSAGE_START`, ...), the deprecated `THINKING_*` last. An event of any other type is skipped by `Run:next()`.",
    );

    m.userdata("Run", |r| {
        r.method(
            "next",
            &[],
            "(Event?, string?)",
            "Wait for the agent's next event: a table with the event's `type` and its fields under their Python SDK names. `(nil, nil)` when the stream has ended: the agent closed it, which after a `RUN_FINISHED` or a `RUN_ERROR` is the end of the run and before one is a run cut short. `(nil, err)` (`agui.next: <kind>: <message>`) when the stream failed (the connection was lost, nothing arrived for `idle_timeout`, an event is not an AG-UI event or is over `max_event`); nothing follows an error. An event of a type the module does not know is skipped. Data inside an event (`snapshot`, `content`, `raw_event`, `value`) is as the agent sent it, with JSON `null` as `nil`.",
        )
        .method(
            "close",
            &[],
            "()",
            "Stop the run: the connection closes, which is how an agent is told to stop, and `next()` returns `(nil, nil)` from then on. Closing a run that has ended does nothing.",
        );
    });

    m.userdata("RunAgentInput", |t| {
        t.field("thread_id", "string", "The conversation the run belongs to.");
        t.field("run_id", "string", "This run's own id.");
        t.field("parent_run_id", "string?", "The run this one continues or branches from.");
        t.field("messages", "{ Message }?", "The whole conversation so far. Activity messages are not sent back to an agent: leave them out.");
        t.field("tools", "{ Tool }?", "The tools the caller offers: the agent asks for one with a tool call, the run ends, and the caller answers with a `tool` message in the next run.");
        t.field("context", "{ Context }?", "Things the agent should know, each a description and a value.");
        t.field("state", "any", "The state the agent starts from.");
        t.field("forwarded_props", "any", "Anything else for the agent, passed through as it is.");
        t.field("resume", "{ { interrupt_id: string, status: string, payload: any, metadata: any } }?", "The answers to the interrupts the last run ended with; `status` is `resolved` or `cancelled`.");
    });
    m.userdata("Message", |t| {
        t.field("id", "string", "The message's id.");
        t.field("role", "string", "`developer`, `system`, `assistant`, `user`, `tool`, `activity` or `reasoning`: which of the other fields the message has.");
        t.field("content", "any", "The text. A `user` or `tool` message may have a list of parts instead (`{ type = \"text\", text }`, `{ type = \"image\" | \"audio\" | \"video\" | \"document\", source = { type = \"data\" | \"url\" | \"file\", value, mime_type } }`); an `activity` message has a table.");
        t.field("name", "string?", "Who wrote it, where the role has more than one author.");
        t.field("tool_calls", "{ ToolCall }?", "`assistant`: the tools the agent asked for.");
        t.field("tool_call_id", "string?", "`tool`: the call this is the result of.");
        t.field("error", "string?", "`tool`: what went wrong, when the tool failed.");
        t.field("activity_type", "string?", "`activity`: what kind of activity `content` describes (`a2ui-surface`, ...).");
        t.field("encrypted_value", "string?", "`reasoning` and `tool`: the provider's opaque form, to be sent back as it is.");
        t.field("metadata", "any", "The sender's own annotations.");
    });
    m.userdata("ToolCall", |t| {
        t.field(
            "id",
            "string",
            "The call's id: a `tool` message answers it by this.",
        );
        t.field("type", "string", "Always `function`.");
        t.field(
            "function",
            "{ name: string, arguments: string }",
            "The tool asked for, and its arguments as JSON text.",
        );
        t.field(
            "encrypted_value",
            "string?",
            "The provider's opaque form, to be sent back as it is.",
        );
    });
    m.userdata("Tool", |t| {
        t.field("name", "string", "The tool's name.");
        t.field("description", "string", "What it does, for the model.");
        t.field("parameters", "any", "The JSON Schema of its arguments.");
    });
    m.userdata("Context", |t| {
        t.field("description", "string", "What the value is.");
        t.field("value", "string", "The value.");
    });
    m.userdata("Interrupt", |t| {
        t.field(
            "id",
            "string",
            "What a resume entry's `interrupt_id` names.",
        );
        t.field(
            "reason",
            "string?",
            "Why the run stopped, in the agent's own word (`tool_approval`, ...).",
        );
        t.field("message", "string?", "The question, for the person.");
        t.field(
            "tool_call_id",
            "string?",
            "The tool call the question is about.",
        );
        t.field(
            "response_schema",
            "any",
            "The JSON Schema the answer's `payload` must fit.",
        );
        t.field(
            "expires_at",
            "string?",
            "When the question stops being answerable.",
        );
        t.field("metadata", "any", "The sender's own annotations.");
        t.field("subagent_run_id", "string?", "The subagent that asked.");
    });
    m.userdata("Event", |t| {
        t.field("type", "string", "Which event this is (one of `EVENT_TYPES`), and so which of the other fields it has.");
        t.field("timestamp", "number?", "When the event was made, in milliseconds since the epoch.");
        t.field("raw_event", "any", "The event of the agent's own framework this one was made from.");
        t.field("metadata", "any", "The sender's own annotations.");
        t.field("subagent_run_id", "string?", "The subagent run the event belongs to (every `SUBAGENT_*` event, and any other event a subagent made).");
        t.field("thread_id", "string?", "`RUN_STARTED`, `RUN_FINISHED`.");
        t.field("run_id", "string?", "`RUN_STARTED`, `RUN_FINISHED`.");
        t.field("parent_run_id", "string?", "`RUN_STARTED`.");
        t.field("input", "RunAgentInput?", "`RUN_STARTED`: the input as the agent read it.");
        t.field("result", "any", "`RUN_FINISHED`, `SUBAGENT_FINISHED`: what the run produced.");
        t.field("usage", "{ { provider: string?, model: string?, input_tokens: number?, output_tokens: number?, total_tokens: number?, reasoning_tokens: number?, cached_input_tokens: number?, cache_write_input_tokens: number? } }?", "`RUN_FINISHED`, `RUN_ERROR`: the tokens each model call used.");
        t.field("message", "string?", "`RUN_ERROR`, `SUBAGENT_ERROR`: what went wrong.");
        t.field("code", "string?", "`RUN_ERROR`, `SUBAGENT_ERROR`: the agent's own code for it.");
        t.field("step_name", "string?", "`STEP_STARTED`, `STEP_FINISHED`.");
        t.field("message_id", "string?", "The message a `TEXT_MESSAGE_*`, `REASONING_*`, `ACTIVITY_*` or `TOOL_CALL_RESULT` event is about. A `*_CHUNK` event has it on the first chunk of a message.");
        t.field("role", "string?", "`TEXT_MESSAGE_START` (`assistant` unless it says otherwise), `REASONING_MESSAGE_START`, `TOOL_CALL_RESULT`, `TEXT_MESSAGE_CHUNK`.");
        t.field("delta", "any", "The next piece of text (`TEXT_MESSAGE_CONTENT`, `REASONING_MESSAGE_CONTENT`, the `*_CHUNK` events, `THINKING_TEXT_MESSAGE_CONTENT`) or of a tool call's JSON arguments (`TOOL_CALL_ARGS`); for `STATE_DELTA`, the JSON Patch to the state.");
        t.field("tool_call_id", "string?", "`TOOL_CALL_START`, `TOOL_CALL_ARGS`, `TOOL_CALL_END`, `TOOL_CALL_RESULT`, `TOOL_CALL_CHUNK`.");
        t.field("tool_call_name", "string?", "`TOOL_CALL_START`, `TOOL_CALL_CHUNK`: the tool asked for.");
        t.field("parent_message_id", "string?", "`TOOL_CALL_START`, `TOOL_CALL_CHUNK`, `SUBAGENT_STARTED`: the assistant message the call belongs to.");
        t.field("content", "any", "`TOOL_CALL_RESULT`: the result, as text. `ACTIVITY_SNAPSHOT`: the activity's content, a table.");
        t.field("snapshot", "any", "`STATE_SNAPSHOT`: the whole state.");
        t.field("messages", "{ Message }?", "`MESSAGES_SNAPSHOT`: the whole conversation as the agent has it.");
        t.field("activity_type", "string?", "`ACTIVITY_SNAPSHOT`, `ACTIVITY_DELTA`: what kind of activity it is.");
        t.field("replace", "boolean?", "`ACTIVITY_SNAPSHOT`: `false` leaves an activity message that exists already as it is.");
        t.field("patch", "{ any }?", "`ACTIVITY_DELTA`: the JSON Patch to the activity's content.");
        t.field("event", "any", "`RAW`: an event of the agent's own framework.");
        t.field("source", "string?", "`RAW`: where it came from.");
        t.field("name", "string?", "`CUSTOM`: the event's name. `SUBAGENT_STARTED`: the subagent's. `TEXT_MESSAGE_START`: the author's.");
        t.field("value", "any", "`CUSTOM`: the event's payload.");
        t.field("subtype", "string?", "`REASONING_ENCRYPTED_VALUE`: `message` or `tool-call`, what `entity_id` names.");
        t.field("entity_id", "string?", "`REASONING_ENCRYPTED_VALUE`.");
        t.field("encrypted_value", "string?", "`REASONING_ENCRYPTED_VALUE`: the provider's opaque form, to be sent back as it is.");
        t.field("description", "string?", "`SUBAGENT_STARTED`.");
        t.field("parent_subagent_run_id", "string?", "`SUBAGENT_STARTED`: the subagent that started this one.");
        t.field("parent_tool_call_id", "string?", "`SUBAGENT_STARTED`: the tool call that started it.");
        t.field("outcome", "{ type: string, interrupts: { Interrupt }?, interrupt_ids: { string }? }?", "How it ended. `RUN_FINISHED`: `type` is `success` (also when there is no outcome) or `interrupt`, with the questions the run stopped at in `interrupts`: the next run answers every one of them in its input's `resume`. `SUBAGENT_FINISHED`: `success`, or `suspended` at the interrupts named in `interrupt_ids`.");
        t.field("protocol_version", "string?", "`RUN_STARTED`: the version of the protocol the agent speaks.");
        t.field("title", "string?", "`THINKING_START`.");
    });

    Ok(m)
}

#[cfg(test)]
mod tests;
