//! Minimal SSE decoder: turns a byte stream into `(event, data)` frames.
//! Server-sent events are `key: value` lines separated by blank lines;
//! we only care about `event:` and `data:`.

use futures::{Stream, StreamExt};

/// One SSE frame: optional event name + the data payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SseFrame {
    pub event: Option<String>,
    pub data: String,
}

/// Decode a reqwest byte stream into SSE frames. Partial frames at EOF
/// are dropped; `data:` lines accumulate with newlines per the spec.
pub fn decode(
    bytes: impl Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
) -> impl Stream<Item = SseFrame> + Send {
    async_stream::stream! {
        futures::pin_mut!(bytes);
        let mut buffer = String::new();
        let mut event: Option<String> = None;
        let mut data = String::new();
        let mut have_data = false;
        while let Some(chunk) = bytes.next().await {
            let Ok(chunk) = chunk else { break };
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            // Consume complete lines.
            while let Some(end) = buffer.find('\n') {
                let line = buffer[..end].trim_end_matches('\r').to_owned();
                buffer.drain(..=end);
                if line.is_empty() {
                    // Blank line: dispatch the frame.
                    if have_data {
                        yield SseFrame { event: event.take(), data: std::mem::take(&mut data) };
                        have_data = false;
                    }
                } else if let Some(value) = line.strip_prefix("event:") {
                    event = Some(value.trim().to_owned());
                } else if let Some(value) = line.strip_prefix("data:") {
                    if have_data {
                        data.push('\n');
                    }
                    data.push_str(value.strip_prefix(' ').unwrap_or(value));
                    have_data = true;
                }
                // `id:`/`:` comment lines are ignored.
            }
        }
        if have_data {
            yield SseFrame { event, data };
        }
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
}
