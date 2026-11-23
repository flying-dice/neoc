//! The events of a run, read from the bytes of its `text/event-stream` response.
//!
//! The SSE framing is `eventsource-stream`'s. Each event's `data` is one JSON [`Event`]; the `event:`, `id:` and
//! `retry:` fields and comment lines are ignored, as the protocol says. An event of a type this crate does not know
//! is skipped.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bytes::Bytes;
use eventsource_stream::{EventStreamError, Eventsource};
use futures_util::{Stream, StreamExt};

use crate::events::Event;

/// The most bytes one event may take on the wire unless the caller says otherwise: 8 MiB.
pub const DEFAULT_MAX_EVENT: usize = 8 << 20;

#[derive(Debug, thiserror::Error)]
pub enum StreamError<E> {
    /// Reading the bytes failed: the caller's own error.
    #[error("{0}")]
    Transport(E),
    /// The bytes are not an event stream (not UTF-8, or a line that is not SSE).
    #[error("not an event stream: {0}")]
    Framing(String),
    /// An event's data is not an AG-UI event.
    #[error("not an AG-UI event: {0}")]
    Event(#[from] serde_json::Error),
    /// One event is longer than the limit.
    #[error("an event is longer than {0} bytes")]
    TooLarge(usize),
}

/// The events in `bytes`, in order. The stream ends when the bytes do, and after the first error.
///
/// `max_event` bounds what is buffered: the parser holds an event until its blank line arrives, so a server that
/// never sends one would otherwise fill the memory.
pub fn events<S, E>(bytes: S, max_event: usize) -> impl Stream<Item = Result<Event, StreamError<E>>>
where
    S: Stream<Item = Result<Bytes, E>>,
{
    // bytes received since the last event came out of the parser: what it can be holding
    let pending = Arc::new(AtomicUsize::new(0));
    let counted = {
        let pending = pending.clone();
        bytes.map(move |chunk| {
            let chunk = chunk.map_err(Fed::Transport)?;
            let held = pending.load(Ordering::Relaxed).saturating_add(chunk.len());
            pending.store(held, Ordering::Relaxed);
            if held > max_event { Err(Fed::TooLarge) } else { Ok(chunk) }
        })
    };
    let mut failed = false;
    counted
        .eventsource()
        .map(move |item| {
            pending.store(0, Ordering::Relaxed);
            match item {
                Ok(sse) => Event::from_json(&sse.data).map_err(StreamError::Event),
                Err(EventStreamError::Transport(Fed::Transport(e))) => Err(StreamError::Transport(e)),
                Err(EventStreamError::Transport(Fed::TooLarge)) => Err(StreamError::TooLarge(max_event)),
                Err(EventStreamError::Utf8(e)) => Err(StreamError::Framing(e.to_string())),
                Err(EventStreamError::Parser(e)) => Err(StreamError::Framing(e.to_string())),
            }
        })
        .take_while(move |item| {
            let go = !failed;
            failed = item.is_err();
            std::future::ready(go)
        })
        .filter_map(|item| std::future::ready(item.transpose()))
}

enum Fed<E> {
    Transport(E),
    TooLarge,
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;

    async fn read(chunks: &[&str], max: usize) -> Vec<Result<Event, String>> {
        let chunks: Vec<Result<Bytes, std::io::Error>> =
            chunks.iter().map(|c| Ok(Bytes::copy_from_slice(c.as_bytes()))).collect();
        let events = events(stream::iter(chunks), max);
        futures_util::pin_mut!(events);
        let mut out = Vec::new();
        while let Some(item) = events.next().await {
            out.push(item.map_err(|e| e.to_string()));
        }
        out
    }

    #[tokio::test]
    async fn events_are_read_across_chunks_and_the_other_sse_fields_are_ignored() {
        let got = read(
            &[
                ": hello\nevent: x\nid: 7\nretry: 5\ndata: {\"type\":\"STEP_STAR",
                "TED\",\"stepName\":\"é\"}\r\n\r\ndata: {\"type\":\"FROM_THE_FUTURE\"}\n\n",
                "data: {\"type\":\"STEP_FINISHED\",\n",
                "data: \"stepName\":\"é\"}\n\n",
            ],
            1024,
        )
        .await;
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(matches!(&got[0], Ok(Event::StepStarted { step_name, .. }) if step_name == "é"));
        assert!(matches!(&got[1], Ok(Event::StepFinished { .. })));
    }

    #[tokio::test]
    async fn the_stream_ends_at_the_first_bad_event() {
        let got = read(&["data: {\"type\":\"STEP_STARTED\"}\n\ndata: {\"type\":\"STEP_STARTED\",\"stepName\":\"s\"}\n\n"], 1024).await;
        assert_eq!(got.len(), 1);
        assert!(got[0].as_ref().is_err_and(|e| e.starts_with("not an AG-UI event")));
    }

    #[tokio::test]
    async fn an_event_that_never_ends_is_cut_at_the_limit() {
        let chunk = format!("data: {}\n", "x".repeat(40));
        let chunks: Vec<&str> = std::iter::repeat_n(chunk.as_str(), 10).collect();
        let got = read(&chunks, 100).await;
        assert_eq!(got, vec![Err("an event is longer than 100 bytes".to_owned())]);
        // many small events never reach it
        let small = "data: {\"type\":\"STEP_STARTED\",\"stepName\":\"s\"}\n\n";
        let chunks: Vec<&str> = std::iter::repeat_n(small, 10).collect();
        assert_eq!(read(&chunks, 100).await.len(), 10);
    }
}
