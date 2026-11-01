//! Minimal HTTP/1.1 parsing and writing.
//!
//! Only what the local editor needs: one request per connection, a bounded
//! header and body, and a plain response. The server binds to loopback, so it
//! is not a general purpose web server.

use std::io::{Read, Write};
use std::net::TcpStream;

/// Largest accepted header block.
pub const MAX_HEADER_BYTES: usize = 64 * 1024;

/// Largest accepted body.
pub const MAX_BODY_BYTES: usize = 1024 * 1024;

/// One parsed request.
#[derive(Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    /// Value of one header, case insensitive.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// Value of one query parameter.
    pub fn query_value(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// Body as text.
    pub fn body_text(&self) -> Result<&str, String> {
        std::str::from_utf8(&self.body).map_err(|_| "body is not UTF-8".to_string())
    }
}

/// One response.
#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Response {
    /// A JSON response.
    pub fn json(status: u16, body: String) -> Self {
        Self {
            status,
            content_type: "application/json; charset=utf-8",
            body: body.into_bytes(),
        }
    }

    /// A plain text message as JSON, for errors.
    pub fn message(status: u16, message: impl AsRef<str>) -> Self {
        let text = serde_json::json!({ "error": message.as_ref() }).to_string();
        Self::json(status, text)
    }

    /// An HTML response.
    pub fn html(body: &str) -> Self {
        Self {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: body.as_bytes().to_vec(),
        }
    }
}

/// Read one request from a connection.
pub fn read_request(stream: &mut TcpStream) -> Result<Request, String> {
    let mut buffer = Vec::with_capacity(2048);
    let mut chunk = [0u8; 4096];

    let header_end = loop {
        if let Some(position) = find(&buffer, b"\r\n\r\n") {
            break position;
        }
        if buffer.len() > MAX_HEADER_BYTES {
            return Err("request headers are too large".into());
        }
        let read = stream.read(&mut chunk).map_err(|e| e.to_string())?;
        if read == 0 {
            return Err("connection closed before the request".into());
        }
        buffer.extend_from_slice(&chunk[..read]);
    };

    let head = std::str::from_utf8(&buffer[..header_end])
        .map_err(|_| "request headers are not UTF-8".to_string())?;
    let mut lines = head.split("\r\n");
    let request_line = lines.next().ok_or("empty request")?;

    let mut parts = request_line.split_whitespace();
    let method = parts.next().ok_or("no method")?.to_string();
    let target = parts.next().ok_or("no target")?;
    let (path, query) = split_target(target);

    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (key, value) = line.split_once(':').ok_or("bad header line")?;
        headers.push((key.trim().to_string(), value.trim().to_string()));
    }

    let content_length = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);

    if content_length > MAX_BODY_BYTES {
        return Err(format!(
            "request body is {content_length} bytes, the limit is {MAX_BODY_BYTES}"
        ));
    }

    let body_start = header_end + 4;
    while buffer.len() < body_start + content_length {
        let read = stream.read(&mut chunk).map_err(|e| e.to_string())?;
        if read == 0 {
            return Err("connection closed before the body".into());
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    let body = buffer[body_start..body_start + content_length].to_vec();

    Ok(Request {
        method,
        path,
        query,
        headers,
        body,
    })
}

/// Write one response and close the connection.
pub fn write_response(stream: &mut TcpStream, response: &Response) -> std::io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "OK",
    };

    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        response.status,
        reason,
        response.content_type,
        response.body.len()
    );

    stream.write_all(head.as_bytes())?;
    stream.write_all(&response.body)?;
    stream.flush()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn split_target(target: &str) -> (String, Vec<(String, String)>) {
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (target, None),
    };

    let mut pairs = Vec::new();
    if let Some(query) = query {
        for pair in query.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (key, value) = match pair.split_once('=') {
                Some((key, value)) => (key, value),
                None => (pair, ""),
            };
            pairs.push((percent_decode(key), percent_decode(value)));
        }
    }

    (percent_decode(path), pairs)
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
            if let Some(value) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(value);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_target_splits_into_a_path_and_a_query() {
        let (path, query) = split_target("/api/state?token=abc&x=1");
        assert_eq!(path, "/api/state");
        assert_eq!(query.len(), 2);
        assert_eq!(query[0], ("token".to_string(), "abc".to_string()));
    }

    #[test]
    fn percent_escapes_are_decoded() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("%2F"), "/");
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(percent_decode("bad%2"), "bad%2");
    }

    #[test]
    fn find_locates_a_terminator() {
        assert_eq!(find(b"abc\r\n\r\nxyz", b"\r\n\r\n"), Some(3));
        assert_eq!(find(b"abc", b"\r\n\r\n"), None);
    }

    #[test]
    fn error_responses_are_json() {
        let response = Response::message(401, "no token");
        assert_eq!(response.status, 401);
        let text = String::from_utf8(response.body).expect("utf8");
        assert!(text.contains("no token"));
    }
}
