//! Minimal SSE decoder, as a three-stage pipeline:
//!
//! ```text
//! bytes ──► lines (buffering) ──► SseLine (pure parse) ──► SseFrame (fold)
//! ```
//!
//! Server-sent events are `field: value` lines separated by blank lines;
//! we only care about `event:` and `data:`, and blank lines dispatch.

use futures::{Stream, StreamExt, future, stream};

/// One SSE frame: optional event name + the data payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SseFrame {
    pub event: Option<String>,
    pub data: String,
}

/// Decode a reqwest byte stream into SSE frames.
pub fn decode(
    bytes: impl Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
) -> impl Stream<Item = SseFrame> + Send {
    // The fold state lives in the closure: filter_map skips `None` (an
    // accumulating line), `scan` would be wrong here — its `None` ENDS
    // the stream (std Iterator::scan semantics).
    let mut frames = FrameBuilder::default();
    lines(bytes)
        .map(|line| parse_line(&line))
        // A trailing dispatch flushes a final frame missing its blank line.
        .chain(stream::once(async { SseLine::Dispatch }))
        .filter_map(move |line| future::ready(frames.feed(line)))
}

/// Stage 1 — bytes to complete lines. The only inherently stateful stage
/// (a partial line can straddle chunk boundaries); kept small on purpose.
fn lines(
    bytes: impl Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send,
) -> impl Stream<Item = String> + Send {
    async_stream::stream! {
        futures::pin_mut!(bytes);
        let mut tail = String::new();
        while let Some(chunk) = bytes.next().await {
            // Transport errors end the stream; the adapter reports the
            // failure at the request level.
            let Ok(chunk) = chunk else { break };
            tail.push_str(&String::from_utf8_lossy(&chunk));
            let mut start = 0;
            while let Some(rel) = tail[start..].find('\n') {
                let end = start + rel;
                yield tail[start..end].trim_end_matches('\r').to_owned();
                start = end + 1;
            }
            tail.drain(..start);
        }
        // A partial tail at EOF is malformed input: dropped.
    }
}

/// Stage 2 — one line, parsed. Pure.
enum SseLine {
    Event(String),
    Data(String),
    /// Blank line: dispatch the accumulated frame.
    Dispatch,
    /// Comments (`:…`) and fields we don't use.
    Skip,
}

fn parse_line(line: &str) -> SseLine {
    if line.is_empty() {
        return SseLine::Dispatch;
    }
    if line.starts_with(':') {
        return SseLine::Skip;
    }
    // "field: value" — the value drops one optional leading space.
    let (field, value) = match line.split_once(':') {
        Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
        None => (line, ""),
    };
    match field {
        "event" => SseLine::Event(value.to_owned()),
        "data" => SseLine::Data(value.to_owned()),
        _ => SseLine::Skip,
    }
}

/// Stage 3 — fold lines into frames: accumulate `data:` lines (joined
/// with newlines per the spec), remember `event:`, dispatch on blank.
/// Frames with no data are dropped.
#[derive(Default)]
struct FrameBuilder {
    event: Option<String>,
    data: Vec<String>,
}

impl FrameBuilder {
    /// Feed one line; `Some(frame)` emerges on dispatch, `None` while
    /// accumulating (filter_map skips, scan would terminate).
    fn feed(&mut self, line: SseLine) -> Option<SseFrame> {
        match line {
            SseLine::Event(name) => {
                self.event = Some(name);
                None
            }
            SseLine::Data(chunk) => {
                self.data.push(chunk);
                None
            }
            SseLine::Skip => None,
            SseLine::Dispatch => self.take(),
        }
    }

    fn take(&mut self) -> Option<SseFrame> {
        (!self.data.is_empty()).then(|| SseFrame {
            event: self.event.take(),
            data: std::mem::take(&mut self.data).join("\n"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream_of(
        chunks: &[&str],
    ) -> impl Stream<Item = Result<bytes::Bytes, reqwest::Error>> + 'static {
        let owned: Vec<String> = chunks.iter().map(|c| c.to_string()).collect();
        futures::stream::iter(owned.into_iter().map(|c| Ok(bytes::Bytes::from(c))))
    }

    #[tokio::test]
    async fn decodes_frames_across_chunk_boundaries() {
        let chunks = [
            "event: mes",
            "sage\ndata: {\"a\":1}\n\nda",
            "ta: second\n\n",
        ];
        let frames: Vec<_> = decode(stream_of(&chunks)).collect().await;
        assert_eq!(
            frames,
            vec![
                SseFrame {
                    event: Some("message".into()),
                    data: "{\"a\":1}".into()
                },
                SseFrame {
                    event: None,
                    data: "second".into()
                },
            ]
        );
    }

    #[tokio::test]
    async fn multi_line_data_joins_with_newlines() {
        let frames: Vec<_> = decode(stream_of(&["data: a\ndata: b\n\n"])).collect().await;
        assert_eq!(frames[0].data, "a\nb");
    }

    #[tokio::test]
    async fn trailing_frame_without_blank_line_is_flushed() {
        let frames: Vec<_> = decode(stream_of(&["data: tail\n"])).collect().await;
        assert_eq!(
            frames,
            vec![SseFrame {
                event: None,
                data: "tail".into()
            }]
        );
    }

    #[tokio::test]
    async fn stage1_lines_split_chunks() {
        let got: Vec<String> = lines(stream_of(&["ev", "ent: a\nda", "ta: x\n\n"]))
            .collect()
            .await;
        assert_eq!(got, vec!["event: a", "data: x", ""]);
    }

    #[test]
    fn stage3_builder_dispatches_on_blank() {
        let mut b = FrameBuilder::default();
        assert_eq!(b.feed(SseLine::Event("m".into())), None);
        assert_eq!(b.feed(SseLine::Data("x".into())), None);
        assert_eq!(
            b.feed(SseLine::Dispatch),
            Some(SseFrame {
                event: Some("m".into()),
                data: "x".into()
            })
        );
    }

    #[test]
    fn parse_line_is_spec_shaped() {
        assert!(matches!(parse_line(""), SseLine::Dispatch));
        assert!(matches!(parse_line(": keepalive"), SseLine::Skip));
        assert!(matches!(parse_line("id: 42"), SseLine::Skip));
        assert!(matches!(parse_line("data: x"), SseLine::Data(d) if d == "x"));
        assert!(matches!(parse_line("data:x"), SseLine::Data(d) if d == "x"));
        assert!(matches!(parse_line("event: m"), SseLine::Event(e) if e == "m"));
        assert!(matches!(parse_line("eventless"), SseLine::Skip));
    }
}
