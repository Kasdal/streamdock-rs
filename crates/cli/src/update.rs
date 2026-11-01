//! Signed update manifest.
//!
//! A release server holds two files:
//!
//! * `latest.json` with the version, the download URL and the notes.
//! * `latest.json.sig` with 64 signature bytes in hex.
//!
//! The signature is Ed25519 over the exact bytes of `latest.json`. The check
//! verifies the signature, then compares the version. It does not download or
//! install anything.
//!
//! The key below is the RFC 8032 test vector. It is a placeholder: replace it
//! with the public key of the release key before a release.

use std::ffi::c_void;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::Deserialize;
use windows_sys::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpReadData,
    WinHttpReceiveResponse, WinHttpSendRequest, WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
    WINHTTP_FLAG_SECURE,
};

/// Placeholder public key, the RFC 8032 test vector. Replace before release.
pub const PUBLIC_KEY_HEX: &str =
    "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";

/// Secret of the test key. Only used by tests and by `update sign`.
#[allow(dead_code)]
pub const TEST_SECRET_HEX: &str =
    "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";

/// Largest manifest that the check accepts.
pub const MAX_MANIFEST: usize = 64 * 1024;

/// What the release server publishes.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub url: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub notes: String,
}

/// Check the signature and parse the manifest.
pub fn verify(
    manifest_bytes: &[u8],
    signature_hex: &str,
    public_key_hex: &str,
) -> Result<Manifest, String> {
    let key_bytes = hex_to_bytes(public_key_hex)?;
    let key_array: [u8; 32] = key_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "the public key must be 32 bytes".to_string())?;
    let key = VerifyingKey::from_bytes(&key_array).map_err(|e| format!("bad public key: {e}"))?;

    let signature_bytes = hex_to_bytes(signature_hex)?;
    let signature_array: [u8; 64] = signature_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "the signature must be 64 bytes".to_string())?;
    let signature = Signature::from_bytes(&signature_array);

    key.verify(manifest_bytes, &signature)
        .map_err(|_| "the signature does not match the manifest".to_string())?;

    serde_json::from_slice(manifest_bytes).map_err(|e| format!("bad manifest: {e}"))
}

/// Sign a manifest with a hex secret. Used by the release tooling.
pub fn sign(manifest_bytes: &[u8], secret_hex: &str) -> Result<String, String> {
    let bytes = hex_to_bytes(secret_hex)?;
    let secret: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| "the secret must be 32 bytes".to_string())?;
    let key = SigningKey::from_bytes(&secret);
    Ok(to_hex(&key.sign(manifest_bytes).to_bytes()))
}

/// True when `candidate` is a later version than `current`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    parse_version(candidate) > parse_version(current)
}

fn parse_version(text: &str) -> (u64, u64, u64) {
    let mut parts = text
        .trim()
        .trim_start_matches('v')
        .split('.')
        .map(|part| {
            part.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<u64>()
                .unwrap_or(0)
        });
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

fn hex_to_bytes(text: &str) -> Result<Vec<u8>, String> {
    let cleaned: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.len() % 2 != 0 {
        return Err("hex text has an odd length".into());
    }
    let bytes = cleaned.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks(2) {
        let hi = (pair[0] as char)
            .to_digit(16)
            .ok_or_else(|| "bad hex digit".to_string())?;
        let lo = (pair[1] as char)
            .to_digit(16)
            .ok_or_else(|| "bad hex digit".to_string())?;
        out.push((hi * 16 + lo) as u8);
    }
    Ok(out)
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Fetch one URL with WinHTTP. Both http and https work.
pub fn fetch(url: &str, max: usize) -> Result<Vec<u8>, String> {
    let (secure, host, port, path) = split_url(url)?;
    let agent = wide("StreamDock");
    let method = wide("GET");
    let host_wide = wide(&host);
    let path_wide = wide(&path);

    // SAFETY: every handle is released by `Handle`, and the buffers live for
    // the whole call.
    unsafe {
        let session = Handle::new(
            WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
                std::ptr::null(),
                std::ptr::null(),
                0,
            ),
            "WinHttpOpen",
        )?;
        let connect = Handle::new(
            WinHttpConnect(session.0, host_wide.as_ptr(), port, 0),
            "WinHttpConnect",
        )?;
        let flags = if secure { WINHTTP_FLAG_SECURE } else { 0 };
        let request = Handle::new(
            WinHttpOpenRequest(
                connect.0,
                method.as_ptr(),
                path_wide.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                flags,
            ),
            "WinHttpOpenRequest",
        )?;

        if WinHttpSendRequest(
            request.0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
            0,
            0,
        ) == 0
        {
            return Err(io_error("WinHttpSendRequest"));
        }
        if WinHttpReceiveResponse(request.0, std::ptr::null_mut()) == 0 {
            return Err(io_error("WinHttpReceiveResponse"));
        }

        let mut body = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            let mut read: u32 = 0;
            if WinHttpReadData(
                request.0,
                buffer.as_mut_ptr() as *mut c_void,
                buffer.len() as u32,
                &mut read,
            ) == 0
            {
                return Err(io_error("WinHttpReadData"));
            }
            if read == 0 {
                break;
            }
            body.extend_from_slice(&buffer[..read as usize]);
            if body.len() > max {
                return Err(format!("the response is larger than {max} bytes"));
            }
        }

        Ok(body)
    }
}

struct Handle(*mut c_void);

impl Handle {
    fn new(handle: *mut c_void, what: &str) -> Result<Self, String> {
        if handle.is_null() {
            Err(io_error(what))
        } else {
            Ok(Self(handle))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            WinHttpCloseHandle(self.0);
        }
    }
}

fn io_error(what: &str) -> String {
    format!("{what} failed: {}", std::io::Error::last_os_error())
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn split_url(url: &str) -> Result<(bool, String, u16, String), String> {
    let (secure, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (true, rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (false, rest)
    } else {
        return Err("the URL must start with http:// or https://".into());
    };

    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };

    let default_port = if secure { 443 } else { 80 };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => (
            host.to_string(),
            port.parse::<u16>()
                .map_err(|_| format!("bad port in {url}"))?,
        ),
        _ => (authority.to_string(), default_port),
    };

    if host.is_empty() {
        return Err(format!("{url} has no host"));
    }

    Ok((secure, host, port, path.to_string()))
}

/// Fetch a manifest and its signature, verify, and compare the version.
pub fn check(url: &str, public_key_hex: &str, current_version: &str) -> Result<Outcome, String> {
    let manifest_bytes = fetch(url, MAX_MANIFEST)?;
    let signature_hex = fetch(&format!("{url}.sig"), MAX_MANIFEST)?;
    let signature_hex = String::from_utf8(signature_hex)
        .map_err(|_| "the signature file is not text".to_string())?;

    let manifest = verify(&manifest_bytes, &signature_hex, public_key_hex)?;
    let newer = is_newer(&manifest.version, current_version);

    Ok(Outcome {
        manifest,
        newer,
        current: current_version.to_string(),
    })
}

/// Result of one update check.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub manifest: Manifest,
    pub newer: bool,
    pub current: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    const MANIFEST: &str = r#"{"version":"0.3.0","url":"https://example.invalid/StreamDock-0.3.0.zip","sha256":"abc","notes":"Faster paint"}"#;

    #[test]
    fn a_signed_manifest_verifies() {
        let signature = sign(MANIFEST.as_bytes(), TEST_SECRET_HEX).expect("sign");
        let manifest = verify(MANIFEST.as_bytes(), &signature, PUBLIC_KEY_HEX).expect("verify");
        assert_eq!(manifest.version, "0.3.0");
        assert!(manifest.notes.contains("Faster"));
    }

    #[test]
    fn a_changed_manifest_fails() {
        let signature = sign(MANIFEST.as_bytes(), TEST_SECRET_HEX).expect("sign");
        let mut tampered = MANIFEST.to_string();
        tampered = tampered.replace("0.3.0", "9.9.9");
        assert!(verify(tampered.as_bytes(), &signature, PUBLIC_KEY_HEX).is_err());
    }

    #[test]
    fn versions_compare_by_number() {
        assert!(is_newer("0.3.0", "0.2.9"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.1.9", "0.2.0"));
        assert!(is_newer("1.0.0-beta", "0.9.0"));
    }

    #[test]
    fn hex_round_trips() {
        assert_eq!(hex_to_bytes("00ff10").expect("hex"), vec![0, 255, 16]);
        assert_eq!(to_hex(&[0, 255, 16]), "00ff10");
        assert!(hex_to_bytes("0").is_err());
        assert!(hex_to_bytes("zz").is_err());
    }

    #[test]
    fn a_url_splits_for_the_fetcher() {
        assert_eq!(
            split_url("https://example.com/a/b").expect("url"),
            (true, "example.com".into(), 443, "/a/b".into())
        );
        assert_eq!(
            split_url("http://127.0.0.1:8080").expect("url"),
            (false, "127.0.0.1".into(), 8080, "/".into())
        );
        assert!(split_url("ftp://example.com").is_err());
    }

    #[test]
    fn a_fetch_reads_a_local_response() {
        let body = MANIFEST;
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buffer = [0u8; 4096];
            let _ = stream.read(&mut buffer);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        });

        let bytes = fetch(&format!("http://127.0.0.1:{port}/latest.json"), MAX_MANIFEST)
            .expect("fetch");
        assert_eq!(bytes, MANIFEST.as_bytes());
        let _ = handle.join();
    }
}
