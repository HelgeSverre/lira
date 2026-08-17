//! Coverage for effectful runtime builtins that the rest of the suite never
//! exercises: HTTP, TCP, env introspection, file seek, chdir, random bytes.
//!
//! Everything here is hermetic — HTTP/TCP run against a localhost server bound
//! to an ephemeral port, file/seek use a unique temp file, and env mutations
//! use a unique key. No external network or fixed paths.

use liravm::runtime::Runtime;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

/// Tiny localhost HTTP/1.1 server that answers `count` requests with
/// `200 OK` + the given body, then exits. Returns the bound port.
fn spawn_http_server(count: usize, body: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind http");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for _ in 0..count {
            let (mut stream, _) = match listener.accept() {
                Ok(c) => c,
                Err(_) => return,
            };
            // Drain the request up to the end of headers so the client's write
            // completes before we respond.
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            // `Connection: close` so the client opens a fresh connection per
            // request, matching this one-request-per-connection server (avoids a
            // keep-alive reuse race under concurrent test load).
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.flush();
        }
    });
    port
}

/// One-shot localhost HTTP server with a caller-selected status and body.
fn spawn_http_response(status: &'static str, body: Vec<u8>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind http");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        let (mut stream, _) = match listener.accept() {
            Ok(c) => c,
            Err(_) => return,
        };
        let mut buf = [0u8; 4096];
        let _ = stream.read(&mut buf);
        let header = format!(
            "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(header.as_bytes());
        let _ = stream.write_all(&body);
        let _ = stream.flush();
    });
    port
}

#[test]
fn http_get_post_request_against_local_server() {
    let port = spawn_http_server(3, "ok");
    let base = format!("http://127.0.0.1:{}/", port);
    let rt = Runtime::new();

    let (status, headers, body) = rt.http_get(&base).expect("http_get");
    assert_eq!(status, 200);
    assert_eq!(body, "ok");
    assert!(
        headers.to_lowercase().contains("content-type"),
        "headers carried: {headers:?}"
    );

    let (status, _h, body) = rt
        .http_post(&base, "payload", "text/plain")
        .expect("http_post");
    assert_eq!(status, 200);
    assert_eq!(body, "ok");

    let (status, body) = rt
        .http_request("PUT", &base, "not a header\n: invalid\nX-Test: 1", "data")
        .expect("http_request");
    assert_eq!(status, 200);
    assert_eq!(body, "ok");
}

#[test]
fn http_get_preserves_error_status_and_body() {
    let port = spawn_http_response("418 I'm a Teapot", b"short and stout".to_vec());
    let rt = Runtime::new();

    let (status, _headers, body) = rt
        .http_get(&format!("http://127.0.0.1:{port}/"))
        .expect("HTTP error status is a response, not a transport error");
    assert_eq!(status, 418);
    assert_eq!(body, "short and stout");
}

#[test]
fn http_get_body_limit_preserves_status_and_clears_body() {
    const MAX_RESPONSE_BODY_BYTES: usize = 10 * 1024 * 1024;
    let port = spawn_http_response(
        "413 Payload Too Large",
        vec![b'x'; MAX_RESPONSE_BODY_BYTES + 1],
    );
    let rt = Runtime::new();

    let (status, _headers, body) = rt
        .http_get(&format!("http://127.0.0.1:{port}/"))
        .expect("body-limit failure occurs after receiving a response");
    assert_eq!(status, 413);
    assert!(
        body.is_empty(),
        "oversize response must not leak a partial body"
    );
}

#[test]
fn http_get_on_closed_port_errors() {
    // Bind then drop to obtain a port that is (almost certainly) refused.
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let rt = Runtime::new();
    let res = rt.http_get(&format!("http://127.0.0.1:{}/", port));
    assert!(res.is_err(), "connect to a closed port should error");
}

#[test]
fn tcp_connect_write_read_close_roundtrip() {
    // Localhost echo server, single connection.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind tcp");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            while let Ok(n) = stream.read(&mut buf) {
                if n == 0 {
                    break;
                }
                if stream.write_all(&buf[..n]).is_err() {
                    break;
                }
                let _ = stream.flush();
            }
        }
    });

    let mut rt = Runtime::new();
    let id = rt.tcp_connect("127.0.0.1", port as i64);
    assert!(id >= 0, "connect should succeed, got {id}");

    assert_eq!(rt.tcp_write(id, "ping\n"), 5);
    assert_eq!(
        rt.tcp_read_line(id),
        "ping\n",
        "echo server returns the line"
    );

    assert_eq!(rt.tcp_write(id, "abc"), 3);
    assert_eq!(rt.tcp_read(id, 64), "abc");

    // Error paths: writing to / closing an unknown socket.
    assert_eq!(rt.tcp_write(99999, "x"), -1);

    assert!(rt.tcp_close(id), "close an open socket");
    assert!(!rt.tcp_close(id), "closing again reports false");
}

#[test]
fn tcp_connect_refused_returns_negative() {
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let mut rt = Runtime::new();
    assert_eq!(rt.tcp_connect("127.0.0.1", port as i64), -1);
}

#[test]
fn env_all_keys_and_exe() {
    let key = "LIRA_COV_TEST_VAR_8421";
    let rt = Runtime::new();
    assert!(rt.env_set(key, "value-xyz"));

    assert!(
        rt.env_all()
            .iter()
            .any(|kv| kv == "LIRA_COV_TEST_VAR_8421=value-xyz"),
        "env_all carries KEY=VALUE pairs"
    );
    assert!(
        rt.env_keys().iter().any(|k| k == key),
        "env_keys lists the name"
    );
    assert!(!rt.env_exe().is_empty(), "env_exe resolves the test binary");

    rt.env_remove(key);
}

#[test]
fn file_seek_repositions_reads() {
    // Unique per process so concurrent test runs (e.g. cargo test + llvm-cov)
    // never collide on a shared temp path.
    let path = std::env::temp_dir().join(format!("lira_cov_seek_{}.txt", std::process::id()));
    let path_str = path.to_str().unwrap();
    let mut rt = Runtime::new();

    let fd = rt.file_open(path_str, 1).expect("open write");
    rt.file_write(fd, "hello world").expect("write");
    rt.file_close(fd).expect("close");

    let fd = rt.file_open(path_str, 0).expect("open read");
    assert_eq!(rt.file_seek(fd, 6, 0).expect("seek set"), 6); // SEEK_SET past "hello "
    assert_eq!(rt.file_read(fd, 64).expect("read"), "world");
    // SEEK_END with a negative offset lands on the last char.
    assert_eq!(rt.file_seek(fd, -1, 2).expect("seek end"), 10);
    assert_eq!(rt.file_read(fd, 64).expect("read tail"), "d");
    assert!(rt.file_seek(fd, 0, 99).is_err(), "invalid whence errors");
    rt.file_close(fd).expect("close");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn os_chdir_rejects_missing_dir_without_moving() {
    // Only the error path: changing the process-wide cwd would race with other
    // tests. A missing target must return false and leave cwd untouched.
    let rt = Runtime::new();
    let before = rt.os_getcwd();
    assert!(!rt.os_chdir("/no/such/dir/lira_cov_8421"));
    assert_eq!(rt.os_getcwd(), before, "cwd unchanged after a failed chdir");
}

#[test]
fn random_bytes_length_and_variation() {
    let rt = Runtime::new();
    assert_eq!(rt.random_bytes(16).len(), 16);
    assert_eq!(rt.random_bytes(0).len(), 0);
    // Two draws of a decent size should differ (vanishing collision odds).
    assert_ne!(rt.random_bytes(32), rt.random_bytes(32));
}

#[test]
fn random_int_full_domain_does_not_overflow() {
    // A request spanning the entire int64 domain must neither overflow nor
    // panic: it must produce a value in [i64::MIN, i64::MAX] for every draw.
    let rt = Runtime::new();
    for _ in 0..100 {
        let value = rt.random_int(i64::MIN, i64::MAX);
        assert!(
            value >= i64::MIN && value <= i64::MAX,
            "out of range: {value}"
        );
    }
}

#[test]
fn random_int_reversed_bounds_return_min() {
    // When `min > max` the contract is to return `min` (matching native).
    let rt = Runtime::new();
    assert_eq!(rt.random_int(5, 3), 5);
    assert_eq!(rt.random_int(123, 123), 123);
    assert_eq!(rt.random_int(i64::MIN, i64::MIN), i64::MIN);
}

#[test]
fn random_int_single_value_range_is_constant() {
    let rt = Runtime::new();
    for _ in 0..100 {
        assert_eq!(rt.random_int(42, 42), 42);
    }
}

#[test]
fn random_int_large_range_stays_bounded() {
    // A large but non-full range must never escape [min, max] (the old
    // float-scaling path lost precision and could overshoot near i64::MAX).
    let rt = Runtime::new();
    for _ in 0..200 {
        let value = rt.random_int(i64::MAX - 10_000, i64::MAX);
        assert!(
            value >= i64::MAX - 10_000 && value <= i64::MAX,
            "escaped range: {value}"
        );
    }
}

#[test]
fn time_from_components_utc_millis_and_fail_closed() {
    let rt = Runtime::new();
    // 2020-01-02 03:04:05 UTC in epoch milliseconds.
    assert_eq!(
        rt.time_from_components(2020, 1, 2, 3, 4, 5),
        1_577_934_245_000
    );
    // Extreme components that cannot be represented must fail closed (0),
    // matching the native backend, rather than silently truncating to a
    // plausible-but-wrong date.
    assert_eq!(
        rt.time_from_components(i64::MIN, 1, 1, 0, 0, 0),
        0,
        "an unrepresentable year must fail closed"
    );
    // Month, day, hour, minute, and second bounds are enforced like the
    // native backend (the native C path used to let timegm normalize these).
    for invalid in [
        (2020i64, 0i64, 1i64, 0i64, 0i64, 0i64), // month 0
        (2020, 13, 1, 0, 0, 0),                  // month 13
        (2021, 2, 29, 0, 0, 0),                  // non-leap Feb 29
        (2020, 2, 30, 0, 0, 0),                  // impossible Feb 30
        (2020, 4, 31, 0, 0, 0),                  // April 31
        (2020, 1, 32, 0, 0, 0),                  // Jan 32
        (2020, 1, 1, 24, 0, 0),                  // hour 24
        (2020, 1, 1, 0, 60, 0),                  // minute 60
        (2020, 1, 1, 0, 0, 60),                  // second 60
    ] {
        assert_eq!(
            rt.time_from_components(
                invalid.0, invalid.1, invalid.2, invalid.3, invalid.4, invalid.5
            ),
            0,
            "invalid date must fail closed: {invalid:?}"
        );
    }
    // Leap year: 2020-02-29 is a real date.
    assert_eq!(
        rt.time_from_components(2020, 2, 29, 0, 0, 0),
        1_582_934_400_000,
        "2020-02-29 is valid (leap year)"
    );
    // chrono (NaiveDate) year bounds: -262143..=262142 inclusive.
    assert_eq!(
        rt.time_from_components(262142, 1, 1, 0, 0, 0),
        8_210_235_340_800_000,
        "262142 is the upper chrono year bound"
    );
    assert_eq!(
        rt.time_from_components(262143, 1, 1, 0, 0, 0),
        0,
        "262143 is outside the chrono year range"
    );
    assert_eq!(
        rt.time_from_components(-262143, 1, 1, 0, 0, 0),
        -8_334_601_228_800_000,
        "-262143 is the lower chrono year bound"
    );
    assert_eq!(
        rt.time_from_components(-262144, 1, 1, 0, 0, 0),
        0,
        "-262144 is outside the chrono year range"
    );
}
