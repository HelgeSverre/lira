//! Blocking I/O syscalls (HTTP) must run in parallel across fibers, not
//! serialize on the single VM thread.
//!
//! A local server delays each response; N fibers each fetch it concurrently.
//! If the fetches overlap (offloaded to the I/O pool), wall-clock ≈ one delay;
//! if they serialized, it would be ≈ N delays. We assert both correctness (all
//! fibers get 200) and a wall-clock well under the serial time.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::{Duration, Instant};

/// A server that handles each connection on its own thread, sleeping `delay`
/// before responding — so N concurrent clients can all be in-flight at once.
fn spawn_slow_server(count: usize, delay: Duration) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for _ in 0..count {
            let (mut stream, _) = match listener.accept() {
                Ok(c) => c,
                Err(_) => return,
            };
            thread::spawn(move || {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                thread::sleep(delay);
                let body = "ok";
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
            });
        }
    });
    port
}

fn run(source: &str) -> Vec<String> {
    let bytecode = lirac::compile_with_imports("test.li", source).expect("compile");
    let (_code, output) = liravm::run_with_capture(&bytecode).expect("run");
    output
}

#[test]
fn http_get_parallelizes_across_fibers() {
    const N: usize = 8;
    let delay = Duration::from_millis(300);
    let port = spawn_slow_server(N, delay);
    let url = format!("http://127.0.0.1:{}/", port);

    let source = format!(
        r#"
fn fetch(url: string, done: Channel<int>) {{
    let r = http_get(url)
    send(done, r[0])
}}
fn main() {{
    let done = chan({n})
    var i = 0
    while i < {n} {{
        spawn fetch("{url}", done)
        i = i + 1
    }}
    var ok = 0
    var got = 0
    while got < {n} {{
        select {{ s = <-done => {{ if s == 200 {{ ok = ok + 1 }} }} }}
        got = got + 1
    }}
    println(ok)
}}
main()
"#,
        n = N,
        url = url
    );

    let start = Instant::now();
    let output = run(&source);
    let elapsed = start.elapsed();

    // Correctness: every fiber got a 200.
    assert_eq!(
        output,
        vec![N.to_string()],
        "all {N} fetches should return 200"
    );

    // Parallelism: overlapped fetches finish in ~one delay, not N delays.
    // Serial would be N * 300ms = 2.4s; require comfortably under that.
    let serial = delay * N as u32;
    assert!(
        elapsed < serial / 2,
        "expected parallel I/O (< {:?}), took {:?} — fetches appear serialized",
        serial / 2,
        elapsed
    );
}

#[test]
fn sleep_parks_the_fiber_and_overlaps() {
    // 8 fibers each sleep 300ms. Offloaded to the I/O pool, they overlap and
    // finish in ~one sleep; the old inline sleep blocked every fiber (serial).
    const N: usize = 8;
    let source = format!(
        r#"
fn nap(done: Channel<int>) {{
    sleep(300)
    send(done, 1)
}}
fn main() {{
    let done = chan({n})
    var i = 0
    while i < {n} {{
        spawn nap(done)
        i = i + 1
    }}
    var got = 0
    while got < {n} {{
        select {{ v = <-done => got = got + 1 }}
    }}
    println(got)
}}
main()
"#,
        n = N
    );

    let start = Instant::now();
    let output = run(&source);
    let elapsed = start.elapsed();

    assert_eq!(output, vec![N.to_string()]);
    assert!(
        elapsed < Duration::from_millis(300 * N as u64) / 2,
        "expected sleeps to overlap, took {:?}",
        elapsed
    );
}

/// A concurrent TCP echo server: each connection handled on its own thread,
/// delaying `delay` before echoing, so N clients can overlap.
fn spawn_echo_server(count: usize, delay: Duration) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for _ in 0..count {
            let (mut stream, _) = match listener.accept() {
                Ok(c) => c,
                Err(_) => return,
            };
            thread::spawn(move || {
                let mut buf = [0u8; 1024];
                let n = stream.read(&mut buf).unwrap_or(0);
                thread::sleep(delay);
                let _ = stream.write_all(&buf[..n]);
                let _ = stream.flush();
            });
        }
    });
    port
}

#[test]
fn concurrent_tcp_overlaps_and_is_correct() {
    const N: usize = 8;
    let delay = Duration::from_millis(300);
    let port = spawn_echo_server(N, delay);

    // Each fiber connects, writes "ping" (4 bytes), reads the echo, and reports
    // the echoed length. Sum proves every socket round-tripped correctly.
    let source = format!(
        r#"
fn work(port: int, done: Channel<int>) {{
    let s = tcp_connect("127.0.0.1", port)
    tcp_write(s, "ping")
    let r = tcp_read(s, 1024)
    tcp_close(s)
    send(done, len(r))
}}
fn main() {{
    let done = chan({n})
    var i = 0
    while i < {n} {{
        spawn work({port}, done)
        i = i + 1
    }}
    var total = 0
    var got = 0
    while got < {n} {{
        select {{ v = <-done => total = total + v }}
        got = got + 1
    }}
    println(total)
}}
main()
"#,
        n = N,
        port = port
    );

    let start = Instant::now();
    let output = run(&source);
    let elapsed = start.elapsed();

    assert_eq!(output, vec![(N * 4).to_string()], "each echo is 4 bytes");
    assert!(
        elapsed < delay * N as u32 / 2,
        "expected TCP reads to overlap, took {:?}",
        elapsed
    );
}

#[test]
fn concurrent_file_io_is_correct() {
    // N fibers each open a distinct temp file, write their id's worth of bytes,
    // close, reopen, read it back, and report the byte count. Exercises the
    // file-handle checkout/offload/re-insert path under concurrency; the sum
    // proves every handle round-tripped through the pool without corruption.
    const N: usize = 8;
    let dir = std::env::temp_dir().join(format!("lira_fileio_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let base = dir.to_string_lossy().to_string();

    // Each fiber writes "payload-<id>" to its own file, reads it back, and
    // reports the byte count. The distinct per-id lengths mean a corrupted or
    // cross-wired handle would change the sum.
    let source = format!(
        r#"
fn work(id: int, dir: string, done: Channel<int>) {{
    let path = dir + "/f" + id + ".txt"
    let payload = "payload-" + id
    let wfd = file_open(path, 1)
    file_write(wfd, payload)
    file_close(wfd)
    let rfd = file_open(path, 0)
    let content = file_read(rfd, 100000)
    file_close(rfd)
    send(done, len(content))
}}
fn main() {{
    let done = chan({n})
    var i = 0
    while i < {n} {{
        spawn work(i, "{base}", done)
        i = i + 1
    }}
    var total = 0
    var got = 0
    while got < {n} {{
        select {{ v = <-done => total = total + v }}
        got = got + 1
    }}
    println(total)
}}
main()
"#,
        n = N,
        base = base
    );

    let output = run(&source);
    // "payload-" (8) + decimal digits of id, for id in 0..8 → each is 9 bytes.
    let expected: i64 = (0..N as i64).map(|k| 8 + k.to_string().len() as i64).sum();
    assert_eq!(output, vec![expected.to_string()]);
    let _ = std::fs::remove_dir_all(&dir);
}
