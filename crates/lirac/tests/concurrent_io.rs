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
    assert_eq!(output, vec![N.to_string()], "all {N} fetches should return 200");

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
