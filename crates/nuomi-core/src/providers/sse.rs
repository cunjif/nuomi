//! Minimal SSE decoding helpers shared by both provider clients.

use futures::{Stream, StreamExt};
use std::pin::Pin;

/// Converts a raw HTTP byte stream into a stream of SSE `data:` payloads.
/// Non-data lines are ignored; the `[DONE]` sentinel is filtered out here —
/// terminal detection stays with the protocol-specific consumer.
pub fn data_payloads(
    body: Pin<Box<dyn Stream<Item = reqwest::Result<bytes::Bytes>> + Send>>,
) -> Pin<Box<dyn Stream<Item = String> + Send>> {
    Box::pin(futures::stream::unfold(
        (body, String::new()),
        |(mut body, mut buffer)| async move {
            loop {
                // Emit every complete `data:` line already buffered.
                if let Some(pos) = buffer.find('\n') {
                    let line: String = buffer.drain(..=pos).collect();
                    let line = line.trim_end_matches(['\n', '\r']);
                    if let Some(data) = line.strip_prefix("data:") {
                        let data = data.trim_start();
                        if data != "[DONE]" && !data.is_empty() {
                            return Some((data.to_string(), (body, buffer)));
                        }
                        continue;
                    }
                    continue;
                }
                match body.next().await {
                    Some(Ok(bytes)) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));
                    }
                    // End of body or transport error: flush trailing line if any.
                    _ => {
                        if buffer.is_empty() {
                            return None;
                        }
                        let rest = std::mem::take(&mut buffer);
                        if let Some(data) = rest.trim().strip_prefix("data:") {
                            let data = data.trim();
                            if !data.is_empty() && data != "[DONE]" {
                                return Some((data.to_string(), (body, buffer)));
                            }
                        }
                        return None;
                    }
                }
            }
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::channel::mpsc;
    use futures::SinkExt;

    async fn bytes_stream(
        chunks: Vec<&str>,
    ) -> Pin<Box<dyn Stream<Item = reqwest::Result<bytes::Bytes>> + Send>> {
        let (mut tx, rx) = mpsc::channel::<reqwest::Result<bytes::Bytes>>(16);
        for c in chunks {
            tx.send(Ok(bytes::Bytes::from(c.to_string())))
                .await
                .unwrap();
        }
        Box::pin(rx)
    }

    #[tokio::test]
    async fn extracts_data_lines_across_chunks() {
        let body = bytes_stream(vec![
            "data: {\"a\":1}\n\nda",
            "ta: {\"b\":2}\n\n: comment\ndata: [DONE]\n",
        ])
        .await;
        let got: Vec<String> = data_payloads(body).collect().await;
        assert_eq!(got, vec!["{\"a\":1}".to_string(), "{\"b\":2}".to_string()]);
    }

    #[tokio::test]
    async fn flushes_trailing_data_without_newline() {
        let body = bytes_stream(vec!["data: tail"]).await;
        let got: Vec<String> = data_payloads(body).collect().await;
        assert_eq!(got, vec!["tail".to_string()]);
    }
}
