//! LSP transport layer: stdio read/write + Content-Length message framing.
//!
//! LSP messages are framed with HTTP-like headers:
//! ```text
//! Content-Length: 1234\r\n
//! \r\n
//! {json body}
//! ```
//! This module provides functions to read and write such frames over
//! async stdin/stdout pipes.

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};

use super::LspError;

/// Read a single LSP message frame from the server's stdout.
///
/// Parses the `Content-Length` header, then reads exactly that many bytes
/// as the JSON body. Returns the raw JSON string (not yet parsed).
pub async fn read_message(reader: &mut BufReader<ChildStdout>) -> Result<String, LspError> {
    let mut content_length: Option<usize> = None;
    let mut header_line = String::new();

    loop {
        header_line.clear();
        let n = reader
            .read_line(&mut header_line)
            .await
            .map_err(|e| LspError::Transport(format!("read header: {e}")))?;

        if n == 0 {
            return Err(LspError::Transport("unexpected EOF reading headers".into()));
        }

        let trimmed = header_line.trim_end_matches(['\r', '\n']);

        if trimmed.is_empty() {
            break;
        }

        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(
                rest.trim()
                    .parse::<usize>()
                    .map_err(|e| LspError::Transport(format!("parse Content-Length: {e}")))?,
            );
        }
    }

    let len = content_length.ok_or_else(|| {
        LspError::Protocol("missing Content-Length header in server message".into())
    })?;

    let mut buf = vec![0u8; len];
    reader
        .read_exact(&mut buf)
        .await
        .map_err(|e| LspError::Transport(format!("read body ({len} bytes): {e}")))?;

    String::from_utf8(buf).map_err(|e| LspError::Protocol(format!("invalid UTF-8 in body: {e}")))
}

/// Write a single LSP message frame to the server's stdin.
///
/// Prepends `Content-Length` header and `\r\n\r\n` separator before the
/// JSON body.
pub async fn write_message(stdin: &mut ChildStdin, json: &str) -> Result<(), LspError> {
    let bytes = json.as_bytes();
    let header = format!("Content-Length: {}\r\n\r\n", bytes.len());

    stdin
        .write_all(header.as_bytes())
        .await
        .map_err(|e| LspError::Transport(format!("write header: {e}")))?;

    stdin
        .write_all(bytes)
        .await
        .map_err(|e| LspError::Transport(format!("write body: {e}")))?;

    stdin
        .flush()
        .await
        .map_err(|e| LspError::Transport(format!("flush: {e}")))?;

    Ok(())
}

/// Encode a JSON-RPC 2.0 message as an LSP frame string (for testing).
pub fn encode_frame(json: &str) -> String {
    format!("Content-Length: {}\r\n\r\n{}", json.len(), json)
}

/// Decode an LSP frame string into its JSON body (for testing).
pub fn decode_frame(frame: &str) -> Result<String, LspError> {
    let separator = "\r\n\r\n";
    let idx = frame
        .find(separator)
        .ok_or_else(|| LspError::Protocol("no header/body separator".into()))?;

    let header = &frame[..idx];
    let body = &frame[idx + separator.len()..];

    let content_length = header
        .strip_prefix("Content-Length:")
        .ok_or_else(|| LspError::Protocol("no Content-Length header".into()))?
        .trim()
        .parse::<usize>()
        .map_err(|e| LspError::Protocol(format!("parse Content-Length: {e}")))?;

    if body.len() != content_length {
        return Err(LspError::Protocol(format!(
            "body length {} != Content-Length {content_length}",
            body.len()
        )));
    }

    Ok(body.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let json = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        let frame = encode_frame(json);
        assert!(frame.starts_with("Content-Length: "));
        assert!(frame.contains("\r\n\r\n"));
        let decoded = decode_frame(&frame).unwrap();
        assert_eq!(decoded, json);
    }

    #[test]
    fn decode_validates_content_length() {
        let frame = "Content-Length: 5\r\n\r\nhello";
        assert_eq!(decode_frame(frame).unwrap(), "hello");
    }

    #[test]
    fn decode_rejects_mismatched_length() {
        let frame = "Content-Length: 10\r\n\r\nshort";
        assert!(decode_frame(frame).is_err());
    }

    #[test]
    fn decode_rejects_missing_separator() {
        let frame = "Content-Length: 5";
        assert!(decode_frame(frame).is_err());
    }

    #[test]
    fn encode_empty_body() {
        let frame = encode_frame("");
        assert_eq!(frame, "Content-Length: 0\r\n\r\n");
        assert_eq!(decode_frame(&frame).unwrap(), "");
    }
}
