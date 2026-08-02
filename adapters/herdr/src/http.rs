//! Minimal loopback HTTP client for capability probing.
//!
//! Herdr's local API is a loopback-only contract (spec §13.6). This module
//! speaks just enough HTTP/1.1 to probe and invoke it without pulling a full
//! HTTP stack into the adapter crates. Only plaintext `http://` loopback
//! targets are supported; TLS is out of scope for a machine-local capability
//! probe.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// A parsed HTTP response: status code plus the body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// Parses a raw HTTP/1.1 response into a status code and body.
///
/// Handles the `Content-Length` header; responses without one fall back to
/// treating everything after the header terminator as the body.
#[must_use]
pub fn parse_http_response(bytes: &[u8]) -> Option<HttpResponse> {
    let split = header_end(bytes)?;
    let (head, body) = bytes.split_at(split);
    let head = std::str::from_utf8(head).ok()?;
    let status = parse_status(head)?;
    let body = match content_length(head) {
        Some(length) => String::from_utf8_lossy(body.get(..length)?).into_owned(),
        None => String::from_utf8_lossy(body).into_owned(),
    };
    Some(HttpResponse { status, body })
}

/// Sends `method path` to a loopback `http://` base URL and returns the body.
///
/// # Errors
///
/// Returns a readable message when the URL is not a plaintext loopback target,
/// the connection fails, or the response cannot be parsed.
pub fn http_request(
    base_url: &str,
    method: &str,
    path: &str,
    timeout: Duration,
) -> Result<HttpResponse, String> {
    let (host, port, base_path) = parse_base_url(base_url)?;
    let addr = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|error| format!("cannot resolve `{host}`: {error}"))?
        .next()
        .ok_or_else(|| format!("no address for `{host}`"))?;

    let mut stream =
        TcpStream::connect_timeout(&addr, timeout).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|error| error.to_string())?;

    let request = format!(
        "{method} {base_path}{path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\nAccept: application/json\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| error.to_string())?;
    stream.flush().map_err(|error| error.to_string())?;

    let mut bytes = Vec::new();
    stream
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    parse_http_response(&bytes).ok_or_else(|| "unparseable HTTP response".to_string())
}

fn header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|pos| pos + 4)
}

fn parse_status(head: &str) -> Option<u16> {
    let status = head.lines().next()?.split_whitespace().nth(1)?;
    status.parse().ok()
}

fn content_length(head: &str) -> Option<usize> {
    head.lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.trim().eq_ignore_ascii_case("content-length") {
                value.trim().parse().ok()
            } else {
                None
            }
        })
        .filter(|length| *length > 0)
}

/// Splits a base URL into a normalized host, port, and path prefix.
///
/// Only plaintext `http://` loopback targets are accepted (`localhost`, the
/// `127/8` range, or `::1`); a remote host is rejected so the local-API
/// contract can never point at the network. The authority is fully validated:
/// bracketed IPv6 literals, ports, user-info rejection, and any junk after a
/// closing `]` are handled. A missing port defaults to `80`.
pub(crate) fn parse_base_url(base_url: &str) -> Result<(String, u16, String), String> {
    let rest = base_url.strip_prefix("http://").ok_or_else(|| {
        format!("unsupported base URL `{base_url}`: only `http://` loopback is supported")
    })?;
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, format!("/{path}")),
        None => (rest, String::new()),
    };
    let (host, port) = parse_authority(authority, base_url)?;
    if !is_loopback_host(&host) {
        return Err(format!(
            "`{base_url}` is not a loopback URL; only localhost, 127/8, or ::1 are allowed"
        ));
    }
    Ok((host, port, path))
}

/// Parses an HTTP authority into a normalized host (brackets stripped) and
/// port. Rejects user-info (`user:pass@`), malformed IPv6 literals, junk after
/// a closing `]`, empty ports, and non-numeric or out-of-range ports.
fn parse_authority(authority: &str, base_url: &str) -> Result<(String, u16), String> {
    if authority.is_empty() {
        return Err(format!("missing host in `{base_url}`"));
    }
    if authority.contains('@') {
        return Err(format!(
            "`{base_url}` must not include user info in the authority"
        ));
    }
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, after_bracket) = rest
            .split_once(']')
            .ok_or_else(|| format!("unterminated IPv6 literal in `{base_url}`"))?;
        if host.is_empty() {
            return Err(format!("empty IPv6 literal in `{base_url}`"));
        }
        let port = match after_bracket {
            "" => 80,
            port_spec => {
                let port_spec = port_spec
                    .strip_prefix(':')
                    .ok_or_else(|| format!("unexpected text after IPv6 literal in `{base_url}`"))?;
                parse_port(port_spec, base_url)?
            }
        };
        Ok((host.to_string(), port))
    } else {
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) => (host, parse_port(port, base_url)?),
            None => (authority, 80),
        };
        if host.is_empty() {
            return Err(format!("missing host in `{base_url}`"));
        }
        Ok((host.to_string(), port))
    }
}

/// Parses a port string into a `u16`, rejecting empty, non-numeric,
/// out-of-range, and zero values (port `0` is not a valid target).
fn parse_port(port: &str, base_url: &str) -> Result<u16, String> {
    if port.is_empty() {
        return Err(format!("empty port in `{base_url}`"));
    }
    let port: u16 = port
        .parse()
        .map_err(|_| format!("invalid port in `{base_url}`"))?;
    if port == 0 {
        return Err(format!("invalid port in `{base_url}`"));
    }
    Ok(port)
}

/// Whether `host` is a loopback target: `localhost`, the `127/8` IPv4 range, or
/// `::1`. The host must already be normalized (no brackets).
#[must_use]
fn is_loopback_host(host: &str) -> bool {
    if host == "localhost" {
        return true;
    }
    host.parse::<std::net::IpAddr>()
        .is_ok_and(|address| address.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status_and_content_length_body() {
        let response = parse_http_response(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 15\r\n\r\n{\"version\":\"1\"}",
        )
        .expect("response parses");

        assert_eq!(response.status, 200);
        assert_eq!(response.body, "{\"version\":\"1\"}");
    }

    #[test]
    fn parses_responses_without_content_length() {
        let response =
            parse_http_response(b"HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\nnope")
                .expect("response parses");

        assert_eq!(response.status, 404);
        assert_eq!(response.body, "nope");
    }

    #[test]
    fn rejects_malformed_responses() {
        assert!(parse_http_response(b"garbage").is_none());
        assert!(parse_http_response(b"HTTP/1.1 200 OK\r\n\r\n").is_some());
    }

    #[test]
    fn rejects_non_loopback_schemes_and_bad_ports() {
        assert!(parse_base_url("https://127.0.0.1:7398").is_err());
        assert!(parse_base_url("herdr://focus").is_err());
        assert!(parse_base_url("http://127.0.0.1:notaport").is_err());
    }

    #[test]
    fn rejects_remote_hosts_even_on_plaintext_http() {
        assert!(parse_base_url("http://example.com").is_err());
        assert!(parse_base_url("http://192.168.1.10:7398").is_err());
        assert!(parse_base_url("http://10.0.0.1:7398").is_err());
        assert!(parse_base_url("http://169.254.169.254").is_err());
    }

    #[test]
    fn accepts_loopback_hosts_including_ipv6() {
        for url in [
            "http://localhost:7398",
            "http://localhost",
            "http://127.0.0.1",
            "http://127.0.0.2:7398",
            "http://[::1]:7398",
            "http://[::1]",
        ] {
            let (host, port, _) =
                parse_base_url(url).unwrap_or_else(|error| panic!("{url}: {error}"));
            assert!(!host.is_empty(), "{url}");
            assert!(port > 0, "{url}");
        }
    }

    #[test]
    fn parses_bracketed_ipv6_with_and_without_port() {
        let (host, port, _) = parse_base_url("http://[::1]:7398").expect("with port");
        assert_eq!(host, "::1", "brackets are stripped for the normalized host");
        assert_eq!(port, 7398);

        let (host, port, _) = parse_base_url("http://[::1]").expect("without port");
        assert_eq!(host, "::1");
        assert_eq!(port, 80, "missing port defaults to 80");
    }

    #[test]
    fn rejects_malformed_authorities() {
        for url in [
            "http://user:pass@127.0.0.1:7398", // user info
            "http://[::1]evil.example",        // junk after the closing bracket
            "http://[::1]:notaport",           // non-numeric port
            "http://[::1]:0",                  // port 0 is invalid
            "http://[::1",                     // unterminated IPv6 literal
            "http://[]:7398",                  // empty IPv6 literal
            "http://127.0.0.1:",               // empty port
            "http://:",                        // empty host and port
        ] {
            assert!(parse_base_url(url).is_err(), "{url} must be rejected");
        }
    }

    #[test]
    fn splits_host_port_and_path_prefix() {
        let (host, port, path) = parse_base_url("http://127.0.0.1:7398/herdr").expect("parses");
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 7398);
        assert_eq!(path, "/herdr");
    }
}
