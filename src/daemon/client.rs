//! Minimal blocking HTTP/1.1 client over the daemon endpoint (unix socket, or loopback TCP on Windows), used by the CLI
//! (`daemon status|stop`) and by tests. Deliberately tiny: one request per
//! connection, `Connection: close`.

use super::endpoint::{self, Stream};
use anyhow::{Context, Result};
use std::io::{Read, Write};
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
    request_with_headers(socket, method, path, token, &[], timeout)
}

/// Like [`request`] with extra request headers (for example the protocol
/// version header).
pub fn request_with_headers(
    socket: &Path,
    method: &str,
    path: &str,
    token: Option<&str>,
    headers: &[(&str, &str)],
    timeout: Duration,
) -> Result<HttpResponse> {
    request_with_body(socket, method, path, token, headers, None, timeout)
}

/// Header in which a client declares its protocol version. Every endpoint
/// except `/status` refuses requests without it.
pub const PROTOCOL_HEADER: &str = "X-Zootree-Protocol-Version";

fn declares_protocol(headers: &[(&str, &str)]) -> bool {
    headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case(PROTOCOL_HEADER))
}

/// Like [`request_with_headers`] with an optional JSON request body. Sends
/// the protocol version header of this build unless `headers` has one.
pub fn request_with_body(
    socket: &Path,
    method: &str,
    path: &str,
    token: Option<&str>,
    headers: &[(&str, &str)],
    body: Option<&str>,
    timeout: Duration,
) -> Result<HttpResponse> {
    let version = super::PROTOCOL_VERSION.to_string();
    let mut all = headers.to_vec();
    if !declares_protocol(headers) {
        all.push((PROTOCOL_HEADER, version.as_str()));
    }
    request_raw(socket, method, path, token, &all, body, timeout)
}

/// Sends exactly the headers given, without the protocol version header.
/// For probing how the Daemon treats a client that omits it.
pub fn request_raw(
    socket: &Path,
    method: &str,
    path: &str,
    token: Option<&str>,
    headers: &[(&str, &str)],
    body: Option<&str>,
    timeout: Duration,
) -> Result<HttpResponse> {
    let body = body.unwrap_or("");
    let mut stream = endpoint::connect(socket)
        .with_context(|| format!("failed to connect to {}", socket.display()))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: zootree\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    if !body.is_empty() {
        head.push_str("Content-Type: application/json\r\n");
    }
    if let Some(token) = token {
        head.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    // From here on the Daemon may already have accepted the request. Any
    // failure is tagged so a caller knows it must not simply send it again.
    let answer = (|| {
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw)?;
        parse_response(&raw)
    })();
    answer.map_err(|e| e.context(RequestSent))
}

/// Marks a failure that happened after the whole request had been written:
/// the Daemon may have accepted it. Find it with
/// `error.downcast_ref::<RequestSent>()`. Failures before that point (no
/// connection, a failed write) do not carry it, and sending the request again
/// is then safe.
#[derive(Debug)]
pub struct RequestSent;

impl std::fmt::Display for RequestSent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the request was sent but no complete answer arrived")
    }
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

/// A non-200 answer to opening the event stream.
#[derive(Debug)]
pub struct HttpStatusError {
    pub status: u16,
    pub body: String,
}

impl HttpStatusError {
    /// The stable `error.code` of the Daemon's error envelope, if the body is one.
    pub fn error_code(&self) -> Option<String> {
        serde_json::from_str::<serde_json::Value>(&self.body)
            .ok()?
            .get("error")?
            .get("code")?
            .as_str()
            .map(str::to_string)
    }
}

impl std::fmt::Display for HttpStatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "status {}: {}", self.status, self.body)
    }
}

impl std::error::Error for HttpStatusError {}

/// One Server-Sent Events frame.
#[derive(Debug, Clone)]
pub struct SseFrame {
    pub event: String,
    pub id: Option<String>,
    pub data: serde_json::Value,
}

/// Blocking reader of `GET /events`. Frames are read one at a time;
/// keep-alive comments are skipped. A read that times out keeps what was
/// already received, so the caller may poll with short timeouts and call
/// again.
#[derive(Debug)]
pub struct EventStream {
    reader: std::io::BufReader<Stream>,
    pending: String,
    /// Partially read chunk-size line.
    line: String,
    /// Partially read chunk payload (plus its trailing CRLF).
    chunk: Vec<u8>,
    /// Payload length of the chunk being read, once its size line is known.
    chunk_len: Option<usize>,
}

impl EventStream {
    /// Opens the stream, declaring this build's protocol version unless
    /// `headers` has one. `last_event_id` resumes after that sequence number.
    /// A non-200 answer is an [`HttpStatusError`] carrying the status and body.
    pub fn connect(
        socket: &Path,
        token: Option<&str>,
        headers: &[(&str, &str)],
        last_event_id: Option<u64>,
    ) -> Result<Self> {
        let version = super::PROTOCOL_VERSION.to_string();
        let mut all = headers.to_vec();
        if !declares_protocol(headers) {
            all.push((PROTOCOL_HEADER, version.as_str()));
        }
        Self::connect_raw(socket, token, &all, last_event_id)
    }

    /// Like [`Self::connect`], but sends exactly the headers given.
    pub fn connect_raw(
        socket: &Path,
        token: Option<&str>,
        headers: &[(&str, &str)],
        last_event_id: Option<u64>,
    ) -> Result<Self> {
        use std::io::BufRead;
        let mut stream = endpoint::connect(socket)
            .with_context(|| format!("failed to connect to {}", socket.display()))?;
        let mut head =
            String::from("GET /events HTTP/1.1\r\nHost: zootree\r\nConnection: close\r\nAccept: text/event-stream\r\n");
        if let Some(token) = token {
            head.push_str(&format!("Authorization: Bearer {token}\r\n"));
        }
        if let Some(id) = last_event_id {
            head.push_str(&format!("Last-Event-ID: {id}\r\n"));
        }
        for (name, value) in headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("\r\n");
        stream.write_all(head.as_bytes())?;
        let mut reader = std::io::BufReader::new(stream);
        let mut status_line = String::new();
        reader.read_line(&mut status_line)?;
        let status = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .context("malformed HTTP status line")?;
        let mut chunked = false;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line)? == 0 {
                anyhow::bail!("connection closed in response head");
            }
            if line == "\r\n" {
                break;
            }
            let lower = line.to_ascii_lowercase();
            chunked |= lower.starts_with("transfer-encoding:") && lower.contains("chunked");
        }
        if status != 200 {
            let mut body = Vec::new();
            let _ = reader
                .get_mut()
                .set_read_timeout(Some(Duration::from_secs(2)));
            let _ = reader.read_to_end(&mut body);
            let body = if chunked {
                decode_chunked(&body).unwrap_or(body)
            } else {
                body
            };
            return Err(HttpStatusError {
                status,
                body: String::from_utf8_lossy(&body).into_owned(),
            }
            .into());
        }
        Ok(Self {
            reader,
            pending: String::new(),
            line: String::new(),
            chunk: Vec::new(),
            chunk_len: None,
        })
    }

    /// Next frame, `Ok(None)` when the server ended the stream. Fails when
    /// nothing arrives within `timeout`; the error then wraps an
    /// `std::io::Error` of kind `WouldBlock` or `TimedOut`, and nothing is lost.
    pub fn next_frame(&mut self, timeout: Duration) -> Result<Option<SseFrame>> {
        use std::io::BufRead;
        // Setting the timeout fails on a socket the peer already closed; the
        // read below then reports the end of the stream.
        let _ = self.reader.get_mut().set_read_timeout(Some(timeout));
        loop {
            while let Some(end) = self.pending.find("\n\n") {
                let block: String = self.pending.drain(..end + 2).collect();
                if let Some(frame) = parse_frame(&block)? {
                    return Ok(Some(frame));
                }
            }
            let payload = match self.chunk_len {
                Some(len) => len,
                None => {
                    if self.reader.read_line(&mut self.line)? == 0 {
                        return Ok(None);
                    }
                    let size = usize::from_str_radix(self.line.trim(), 16)
                        .with_context(|| format!("bad chunk size {:?}", self.line))?;
                    self.line.clear();
                    if size == 0 {
                        return Ok(None);
                    }
                    self.chunk.clear();
                    self.chunk_len = Some(size);
                    size
                }
            };
            let wanted = payload + 2;
            while self.chunk.len() < wanted {
                let mut buf = [0u8; 4096];
                let take = (wanted - self.chunk.len()).min(buf.len());
                let read = std::io::Read::read(&mut self.reader, &mut buf[..take])?;
                if read == 0 {
                    return Ok(None);
                }
                self.chunk.extend_from_slice(&buf[..read]);
            }
            self.chunk_len = None;
            self.pending
                .push_str(&String::from_utf8_lossy(&self.chunk[..payload]).replace("\r\n", "\n"));
            self.chunk.clear();
        }
    }
}

fn parse_frame(block: &str) -> Result<Option<SseFrame>> {
    let (mut event, mut id, mut data) = (None, None, Vec::new());
    for line in block.lines() {
        if line.starts_with(':') || line.is_empty() {
            continue;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => event = Some(value.to_string()),
            "id" => id = Some(value.to_string()),
            "data" => data.push(value.to_string()),
            _ => {}
        }
    }
    let Some(event) = event else { return Ok(None) };
    Ok(Some(SseFrame {
        event,
        id,
        data: serde_json::from_str(&data.join("\n")).context("event data is not JSON")?,
    }))
}
