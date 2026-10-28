//! Minimal Server-Sent Events splitter (text/event-stream), shared by the
//! OpenAI-compatible and Anthropic backends.

use futures_util::StreamExt;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
}

#[derive(Default)]
pub struct SseParser {
    buf: String,
    event: String,
    data: Vec<String>,
}

impl SseParser {
    /// Feed bytes; returns the events completed by them.
    pub fn feed(&mut self, chunk: &str) -> Vec<SseEvent> {
        self.buf.push_str(chunk);
        let mut out = Vec::new();
        while let Some(pos) = self.buf.find('\n') {
            let mut line: String = self.buf.drain(..=pos).collect();
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
            if line.is_empty() {
                if !self.data.is_empty() || !self.event.is_empty() {
                    out.push(SseEvent { event: std::mem::take(&mut self.event), data: self.data.join("\n") });
                    self.data.clear();
                }
            } else if let Some(v) = line.strip_prefix("data:") {
                self.data.push(v.strip_prefix(' ').unwrap_or(v).to_string());
            } else if let Some(v) = line.strip_prefix("event:") {
                self.event = v.trim().to_string();
            }
            // id:, retry: and comments (":") are ignored.
        }
        out
    }
    /// End of stream: flush an event that lacked its blank line.
    pub fn finish(&mut self) -> Option<SseEvent> {
        if !self.buf.is_empty() {
            let rest = std::mem::take(&mut self.buf);
            let mut ev = self.feed(&(rest + "\n\n"));
            return ev.pop();
        }
        if self.data.is_empty() {
            None
        } else {
            Some(SseEvent { event: std::mem::take(&mut self.event), data: std::mem::take(&mut self.data).join("\n") })
        }
    }
}

/// Pull-based event reader over a streaming HTTP body.
pub struct SseStream {
    body: std::pin::Pin<Box<dyn futures_util::Stream<Item = reqwest::Result<bytes::Bytes>> + Send>>,
    parser: SseParser,
    queue: std::collections::VecDeque<SseEvent>,
    pending: Vec<u8>,
    ended: bool,
}

impl SseStream {
    pub fn new(resp: reqwest::Response) -> Self {
        SseStream { body: Box::pin(resp.bytes_stream()), parser: SseParser::default(), queue: Default::default(), pending: Vec::new(), ended: false }
    }

    pub async fn next(&mut self) -> Option<Result<SseEvent, super::LlmError>> {
        loop {
            if let Some(ev) = self.queue.pop_front() {
                return Some(Ok(ev));
            }
            if self.ended {
                return None;
            }
            match self.body.next().await {
                Some(Ok(chunk)) => {
                    self.pending.extend_from_slice(&chunk);
                    // Only hand complete UTF-8 to the parser.
                    let valid = match std::str::from_utf8(&self.pending) {
                        Ok(_) => self.pending.len(),
                        Err(e) => e.valid_up_to(),
                    };
                    let text = String::from_utf8_lossy(&self.pending[..valid]).into_owned();
                    self.pending.drain(..valid);
                    self.queue.extend(self.parser.feed(&text));
                }
                Some(Err(e)) => return Some(Err(super::LlmError::Http(e.to_string()))),
                None => {
                    self.ended = true;
                    self.queue.extend(self.parser.finish());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn splits_events_across_chunks() {
        let mut p = SseParser::default();
        assert!(p.feed("event: message_start\ndata: {\"a\"").is_empty());
        let ev = p.feed(":1}\n\ndata: [DONE]\r\n\r\n");
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].event, "message_start");
        assert_eq!(ev[0].data, "{\"a\":1}");
        assert_eq!(ev[1].data, "[DONE]");
    }
    #[test]
    fn flushes_unterminated_event() {
        let mut p = SseParser::default();
        assert!(p.feed("data: tail").is_empty());
        assert_eq!(p.finish().unwrap().data, "tail");
    }
}
