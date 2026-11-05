//! Minimal blocking HTTP/1.1 client over a unix socket, used by the CLI
//! (`daemon status|stop`) and by tests. Deliberately tiny: one request per
//! connection, `Connection: close`.

use anyhow::{Context, Result};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

#[derive(Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

impl HttpResponse {
    pub fn json(&self) -> Result<serde_json::Value> {
        serde_json::from_str(&self.body)
            .with_context(|| format!("response is not JSON: {}", self.body))
    }
}

pub fn request(
    socket: &Path,
    method: &str,
    path: &str,
    token: Option<&str>,
    timeout: Duration,
) -> Result<HttpResponse> {
    let mut stream = UnixStream::connect(socket)
        .with_context(|| format!("failed to connect to {}", socket.display()))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: zootree\r\nConnection: close\r\nContent-Length: 0\r\n"
    );
    if let Some(token) = token {
        head.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    parse_response(&raw)
}

fn parse_response(raw: &[u8]) -> Result<HttpResponse> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .context("malformed HTTP response")?;
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let body = &raw[split + 4..];
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .context("malformed HTTP status line")?;
    let chunked = head
        .lines()
        .any(|l| l.to_ascii_lowercase().starts_with("transfer-encoding:") && l.contains("chunked"));
    let body = if chunked {
        decode_chunked(body)?
    } else {
        body.to_vec()
    };
    Ok(HttpResponse {
        status,
        body: String::from_utf8_lossy(&body).to_string(),
    })
}

fn decode_chunked(mut data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let line_end = data
            .windows(2)
            .position(|w| w == b"\r\n")
            .context("malformed chunked body")?;
        let size = usize::from_str_radix(
            std::str::from_utf8(&data[..line_end])?
                .split(';')
                .next()
                .unwrap_or("")
                .trim(),
            16,
        )?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if data.len() < size + 2 {
            anyhow::bail!("truncated chunked body");
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
}
