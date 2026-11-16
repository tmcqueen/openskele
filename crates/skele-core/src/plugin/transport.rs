//! LSP-style framing: `Content-Length: N\r\n\r\n` followed by N bytes of JSON.

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Upper bound on a single frame, so a corrupt header cannot exhaust memory.
const MAX_FRAME: usize = 64 * 1024 * 1024;

/// Reads one frame. `Ok(None)` means the stream ended cleanly between frames.
///
/// A plugin may print to stdout by accident between frames. Such lines, which
/// are neither blank nor one of the two LSP headers, are passed to `on_stray`
/// (without their line ending) and the stream stays usable.
pub async fn read_frame<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    on_stray: &mut impl FnMut(&str),
) -> std::io::Result<Option<Vec<u8>>> {
    let mut length = None;
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).await? == 0 {
            return Ok(None);
        }
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            if let Some(length) = length {
                let mut body = vec![0; length];
                reader.read_exact(&mut body).await?;
                return Ok(Some(body));
            }
            continue;
        }
        match header.split_once(':') {
            Some((name, value)) if name.trim().eq_ignore_ascii_case("content-length") => {
                let parsed: usize = value.trim().parse().map_err(|_| invalid("bad Content-Length"))?;
                if parsed > MAX_FRAME {
                    return Err(invalid("frame too large"));
                }
                length = Some(parsed);
            }
            Some((name, _)) if name.trim().eq_ignore_ascii_case("content-type") => {}
            _ => on_stray(header),
        }
    }
}

pub async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, body: &[u8]) -> std::io::Result<()> {
    writer.write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes()).await?;
    writer.write_all(body).await?;
    writer.flush().await
}

fn invalid(message: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn frames_round_trip_and_stray_output_is_reported() {
        let mut wire = Vec::new();
        write_frame(&mut wire, b"{\"a\":1}").await.unwrap();
        wire.extend_from_slice(b"stray console.log output\n");
        wire.extend_from_slice(b"note: looks like a header\r\n");
        wire.extend_from_slice(b"content-length: 2\r\nContent-Type: application/json\r\n\r\n{}");

        let mut stray = Vec::new();
        let mut on_stray = |line: &str| stray.push(line.to_owned());
        let mut reader = wire.as_slice();
        assert_eq!(read_frame(&mut reader, &mut on_stray).await.unwrap().unwrap(), b"{\"a\":1}");
        assert_eq!(read_frame(&mut reader, &mut on_stray).await.unwrap().unwrap(), b"{}");
        assert!(read_frame(&mut reader, &mut on_stray).await.unwrap().is_none());
        assert_eq!(stray, ["stray console.log output", "note: looks like a header"]);
    }

    #[tokio::test]
    async fn truncated_body_is_an_error() {
        let mut reader: &[u8] = b"Content-Length: 10\r\n\r\n{}";
        assert!(read_frame(&mut reader, &mut |_| {}).await.is_err());
    }
}
