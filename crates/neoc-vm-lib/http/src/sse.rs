//! The server-sent events parser of the HTML standard (§9.2.6, "Interpreting an event stream"), field by field.
//!
//! It is written here rather than taken from a crate because a reconnecting stream needs what a dispatching parser
//! hides: an `id:` and a `retry:` take effect when their line is read, whether or not an event is dispatched after
//! them, and an empty `id:` resets the last event id (which an event's empty id cannot tell apart from "no id").

use std::time::Duration;

/// What a line of the stream does, in the order the lines came.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Item {
    /// An `id:` field: the last event id is now this (`""` resets it).
    Id(String),
    /// A `retry:` field: the reconnection time is now this.
    Retry(Duration),
    /// A blank line ended an event with data: dispatch it, with the last event id as it is now.
    Event {
        /// The `event:` field, or `"message"`.
        event: String,
        /// The `data:` lines, joined with `"\n"`.
        data: String,
    },
}

/// The parser's state across chunks: the line not ended yet, and the event being built.
#[derive(Debug, Default)]
pub(crate) struct Parser {
    pending: Vec<u8>,
    /// The last chunk ended with `\r`: a `\n` that starts the next one belongs to it.
    after_cr: bool,
    /// The stream's first bytes were looked at for a byte order mark.
    started: bool,
    event: String,
    data: String,
}

impl Parser {
    /// Feed the next piece of the stream; what its complete lines did.
    pub(crate) fn feed(&mut self, mut chunk: &[u8]) -> Vec<Item> {
        let mut items = Vec::new();
        if self.after_cr {
            self.after_cr = false;
            if chunk.first() == Some(&b'\n') {
                chunk = &chunk[1..];
            }
        }
        let mut start = 0;
        let mut at = 0;
        while at < chunk.len() {
            match chunk[at] {
                b'\n' | b'\r' => {
                    self.pending.extend_from_slice(&chunk[start..at]);
                    let line = std::mem::take(&mut self.pending);
                    self.line(&line, &mut items);
                    if chunk[at] == b'\r' {
                        if at + 1 == chunk.len() {
                            self.after_cr = true;
                        } else if chunk[at + 1] == b'\n' {
                            at += 1;
                        }
                    }
                    start = at + 1;
                }
                _ => {}
            }
            at += 1;
        }
        self.pending.extend_from_slice(&chunk[start..]);
        items
    }

    fn line(&mut self, line: &[u8], items: &mut Vec<Item>) {
        let mut line = line;
        if !self.started {
            self.started = true;
            line = line.strip_prefix("\u{feff}".as_bytes()).unwrap_or(line);
        }
        if line.is_empty() {
            if !self.data.is_empty() {
                self.data.pop(); // the last data line's "\n"
                let event = if self.event.is_empty() {
                    "message".to_owned()
                } else {
                    std::mem::take(&mut self.event)
                };
                items.push(Item::Event { event, data: std::mem::take(&mut self.data) });
            }
            self.event.clear();
            self.data.clear();
            return;
        }
        if line.first() == Some(&b':') {
            return;
        }
        let (field, value) = match line.iter().position(|&b| b == b':') {
            Some(colon) => {
                let value = &line[colon + 1..];
                (&line[..colon], value.strip_prefix(b" ").unwrap_or(value))
            }
            None => (line, &[][..]),
        };
        let value = String::from_utf8_lossy(value);
        match field {
            b"event" => self.event = value.into_owned(),
            b"data" => {
                self.data.push_str(&value);
                self.data.push('\n');
            }
            b"id" if !value.contains('\0') => items.push(Item::Id(value.into_owned())),
            b"retry" if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) => {
                if let Ok(millis) = value.parse::<u64>() {
                    items.push(Item::Retry(Duration::from_millis(millis)));
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(event: &str, data: &str) -> Item {
        Item::Event { event: event.to_owned(), data: data.to_owned() }
    }

    #[test]
    fn fields_take_effect_as_they_are_read() {
        let mut parser = Parser::default();
        let items =
            parser.feed(b"retry: 50\nid: 1\nevent: greet\ndata: a\ndata: b\n\nid\n\nretry: x\nretry: 7\n\n");
        assert_eq!(
            items,
            [
                Item::Retry(Duration::from_millis(50)),
                Item::Id("1".to_owned()),
                event("greet", "a\nb"),
                // an `id` with no value resets the last id, with no event
                Item::Id(String::new()),
                // a `retry` that is not digits is ignored; one alone still counts
                Item::Retry(Duration::from_millis(7)),
            ]
        );
    }

    #[test]
    fn every_line_end_works_across_chunks_and_an_unended_event_is_not_dispatched() {
        let mut parser = Parser::default();
        let mut items = parser.feed("\u{feff}data: one\r".as_bytes());
        items.extend(parser.feed(b"\n\r\ndata:two\r\r: a comment\ndata"));
        items.extend(parser.feed(b"\n\ndata: cut"));
        assert_eq!(items, [event("message", "one"), event("message", "two"), event("message", "")]);
    }

    #[test]
    fn an_id_with_a_nul_is_ignored_and_a_blank_line_without_data_dispatches_nothing() {
        let mut parser = Parser::default();
        assert_eq!(parser.feed(b"id: a\0b\nevent: x\n\ndata\n\n"), [event("message", "")]);
    }
}
