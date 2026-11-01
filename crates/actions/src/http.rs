//! Minimal HTTP client for the `http` action.
//!
//! Only `http://` is supported. The request is one short exchange on a plain
//! socket with a hard timeout, which is enough for a local integration such as
//! OBS or a home automation hub.
//!
//! HTTPS needs a TLS stack. It is not supported yet, and the profile
//! validation says so.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::{ActionError, Result};

/// Connect, send and read timeout.
pub const TIMEOUT: Duration = Duration::from_secs(3);

/// Largest response that is read.
pub const MAX_RESPONSE: u64 = 1024 * 1024;

/// Send one request and return the status code.
pub fn request(method: &str, url: &str, body: Option<&str>) -> Result<u16> {
    let (host, port, path) = parse_url(url)?;
    let method = method.to_ascii_uppercase();
    let payload = body.unwrap_or("");

    let address = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| ActionError::Os(format!("cannot resolve {host}: {e}")))?
        .next()
        .ok_or_else(|| ActionError::Os(format!("{host} has no address")))?;

    let mut stream = TcpStream::connect_timeout(&address, TIMEOUT)
        .map_err(|e| ActionError::Os(format!("cannot connect to {host}:{port}: {e}")))?;
    stream.set_read_timeout(Some(TIMEOUT)).ok();
    stream.set_write_timeout(Some(TIMEOUT)).ok();

    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\n\r\n{payload}",
        payload.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| ActionError::Os(format!("cannot send to {host}: {e}")))?;

    let mut response = String::new();
    stream
        .take(MAX_RESPONSE)
        .read_to_string(&mut response)
        .map_err(|e| ActionError::Os(format!("cannot read from {host}: {e}")))?;

    let status = response
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| ActionError::Os(format!("bad HTTP response from {url}")))?;

    if (200..300).contains(&status) {
        Ok(status)
    } else {
        Err(ActionError::Os(format!("HTTP {status} from {url}")))
    }
}

/// Split a URL into host, port and path.
pub fn parse_url(url: &str) -> Result<(String, u16, String)> {
    let rest = url.strip_prefix("http://").ok_or_else(|| {
        ActionError::Unsupported(format!("{url} is not an http:// URL; HTTPS is not supported"))
    })?;

    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };

    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            let port = port
                .parse::<u16>()
                .map_err(|_| ActionError::Os(format!("bad port in {url}")))?;
            (host.to_string(), port)
        }
        _ => (authority.to_string(), 80),
    };

    if host.is_empty() {
        return Err(ActionError::Os(format!("{url} has no host")));
    }

    Ok((host, port, path.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    fn serve_once(response: &'static str) -> (u16, thread::JoinHandle<String>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buffer = [0u8; 4096];
            let read = stream.read(&mut buffer).unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            stream.write_all(response.as_bytes()).expect("write");
            request
        });
        (port, handle)
    }

    #[test]
    fn a_get_returns_the_status() {
        let (port, handle) = serve_once("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
        let status = request("GET", &format!("http://127.0.0.1:{port}/ping"), None).expect("request");
        assert_eq!(status, 200);
        let request = handle.join().expect("thread");
        assert!(request.starts_with("GET /ping HTTP/1.1"));
        assert!(request.contains("Host: 127.0.0.1"));
    }

    #[test]
    fn a_post_sends_the_body() {
        let (port, handle) = serve_once("HTTP/1.1 204 No Content\r\n\r\n");
        let status = request(
            "POST",
            &format!("http://127.0.0.1:{port}/hook"),
            Some("hello"),
        )
        .expect("request");
        assert_eq!(status, 204);
        let request = handle.join().expect("thread");
        assert!(request.contains("Content-Length: 5"));
        assert!(request.ends_with("hello"));
    }

    #[test]
    fn an_error_status_is_a_failure() {
        let (port, handle) = serve_once("HTTP/1.1 500 Server Error\r\n\r\n");
        assert!(request("GET", &format!("http://127.0.0.1:{port}/x"), None).is_err());
        let _ = handle.join();
    }

    #[test]
    fn urls_split_into_host_port_and_path() {
        assert_eq!(
            parse_url("http://127.0.0.1:8080/hook").expect("url"),
            ("127.0.0.1".to_string(), 8080, "/hook".to_string())
        );
        assert_eq!(
            parse_url("http://example.com").expect("url"),
            ("example.com".to_string(), 80, "/".to_string())
        );
        assert!(parse_url("https://example.com").is_err());
        assert!(parse_url("http://").is_err());
    }
}
