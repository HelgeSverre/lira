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
        .http_request("PUT", &base, "X-Test: 1", "data")
        .expect("http_request");
    assert_eq!(status, 200);
    assert_eq!(body, "ok");
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
