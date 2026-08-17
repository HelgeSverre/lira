//! End-to-end tests for the native backend.
//!
//! Each test compiles a Lira program to a real executable, runs it, and checks
//! its output. Going through the linker rather than the JIT is deliberate: it
//! covers object emission and linking too, and it keeps the C runtime's
//! single-threaded scheduler state in a process of its own, which the test
//! harness's thread pool would otherwise share.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};

mod common;

/// Compile `source` and return everything it wrote to stdout and stderr.
fn run_native(source: &str) -> Result<String, String> {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let source_path = dir.join("program.li");
    std::fs::write(&source_path, source).expect("write source");

    let result = (|| {
        let output = common::run_aot(&source_path, source)?;
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Ok(text)
    })();

    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// Compile and run a repository source file without changing its path. Keeping
/// the real path is important for `import std.sync`, whose module loader finds
/// `stdlib/` relative to the importing file.
fn run_native_file(source_path: &Path) -> Result<String, String> {
    let source = std::fs::read_to_string(source_path)
        .map_err(|e| format!("could not read {}: {}", source_path.display(), e))?;
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("scratch dir");

    let result = (|| {
        let output = common::run_aot(source_path, &source)?;
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Ok(text)
    })();

    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// Run source through the bytecode VM so Any regressions can assert the same
/// observable output on both backends.
fn run_bytecode(source: &str) -> Result<String, String> {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let source_path = dir.join("program.li");
    std::fs::write(&source_path, source).expect("write source");
    let result = (|| {
        let bytecode =
            lirac::compile_with_imports(source_path.to_str().expect("utf-8 path"), source)?;
        let (exit_code, lines) = liravm::run_with_capture(&bytecode)?;
        if exit_code != 0 {
            return Err(format!("bytecode VM exited with status {}", exit_code));
        }
        Ok(lines.join("\n"))
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// Run source through a VM whose select arbiter starts from an explicit seed.
/// This avoids process-global environment mutation in the parallel integration
/// test binary while still exercising compiled bytecode.
fn run_bytecode_with_select_seed(source: &str, seed: u64) -> Result<String, String> {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let source_path = dir.join("program.li");
    std::fs::write(&source_path, source).expect("write source");
    let result = (|| {
        let bytecode =
            lirac::compile_with_imports(source_path.to_str().expect("utf-8 path"), source)?;
        let program = liravm::bytecode::load(&bytecode)?;
        let mut vm = liravm::VM::new(program);
        vm.set_fiber_mode(true);
        vm.set_capture_output(true);
        vm.set_select_seed(seed);
        let status = vm.run()?;
        if status != 0 {
            return Err(format!("bytecode VM exited with status {status}"));
        }
        Ok(vm.get_output().join("\n"))
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

#[track_caller]
fn assert_seeded_select_aot_vm(source: &str, cases: &[(u64, &[&str])]) {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("seeded select AOT scratch directory");
    let source_path = dir.join("program.li");
    std::fs::write(&source_path, source).expect("seeded select AOT source");
    for (seed, expected) in cases {
        let seed_text = seed.to_string();
        let output = common::run_aot_with_env(&source_path, source, "LIRA_SELECT_SEED", &seed_text)
            .unwrap_or_else(|error| panic!("run seeded AOT binary: {error}"));
        assert!(
            !output.timed_out,
            "seed {seed} exceeded the native child deadline"
        );
        output
            .assert_complete_output()
            .unwrap_or_else(|error| panic!("seed {seed}: {error}"));
        assert!(
            output.status.success(),
            "seed {seed} failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let native = String::from_utf8_lossy(&output.stdout);
        let native_lines: Vec<&str> = native.lines().collect();
        assert_eq!(
            native_lines, *expected,
            "seed {seed}\n--- program ---\n{source}"
        );

        let bytecode = run_bytecode_with_select_seed(source, *seed)
            .unwrap_or_else(|error| panic!("seeded bytecode VM failed: {error}"));
        assert_eq!(
            bytecode.lines().collect::<Vec<_>>(),
            *expected,
            "VM seed {seed}\n--- program ---\n{source}"
        );
        assert_eq!(native.trim_end(), bytecode.trim_end(), "seed {seed}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

fn scratch_dir() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("lira-native-test-{}-{}", std::process::id(), id))
}

/// Start one hermetic localhost server for a native HTTP source test.
///
/// The server handles one request per expected call. It reads exactly the
/// advertised `Content-Length` after the header terminator; waiting for EOF
/// here would deadlock clients that keep the connection open for the response.
fn start_http_server(expected_requests: usize) -> (String, JoinHandle<Vec<Vec<u8>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind localhost test server");
    let address = listener.local_addr().expect("test server address");
    let handle = thread::spawn(move || {
        let mut requests = Vec::with_capacity(expected_requests);
        for _ in 0..expected_requests {
            let (mut stream, _) = listener.accept().expect("accept localhost request");
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .expect("set request timeout");
            let request = read_http_request(&mut stream).expect("read complete HTTP request");
            let request_line = String::from_utf8_lossy(&request)
                .lines()
                .next()
                .unwrap_or_default()
                .to_owned();
            let response = match request_line.as_str() {
                line if line.starts_with("GET /get ") => {
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 7\r\nConnection: close\r\n\r\nmissing"
                }
                line if line.starts_with("POST /post ") => {
                    "HTTP/1.1 201 Created\r\nContent-Length: 7\r\nConnection: close\r\n\r\ncreated"
                }
                line if line.starts_with("PATCH /request ") => {
                    "HTTP/1.1 202 Accepted\r\nContent-Length: 7\r\nConnection: close\r\n\r\npatched"
                }
                line if line.starts_with("GET /jit ") => {
                    "HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\njit"
                }
                _ => "HTTP/1.1 500 Unexpected Request\r\nContent-Length: 8\r\nConnection: close\r\n\r\ninvalid",
            };
            stream
                .write_all(response.as_bytes())
                .expect("write HTTP response");
            stream.flush().expect("flush HTTP response");
            requests.push(request);
        }
        requests
    });
    (format!("http://{address}"), handle)
}

fn start_http_body_server(status: u16, body_len: usize) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind HTTP body server");
    let address = listener.local_addr().expect("HTTP body server address");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept HTTP body request");
        let _ = read_http_request(&mut stream).expect("read HTTP body request");
        let header = format!(
            "HTTP/1.1 {status} Test\r\nContent-Length: {body_len}\r\nConnection: close\r\n\r\n"
        );
        stream
            .write_all(header.as_bytes())
            .expect("write HTTP body header");
        let body = vec![b'x'; body_len];
        let _ = stream.write_all(&body);
    });
    (format!("http://{address}"), handle)
}

/// Delayed concurrent localhost server used to prove HTTP calls do not occupy
/// the scheduler thread. Each accepted request gets its own OS thread.
fn start_slow_http_server(expected_requests: usize) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind slow HTTP server");
    let address = listener.local_addr().expect("slow HTTP server address");
    let handle = thread::spawn(move || {
        let mut workers = Vec::with_capacity(expected_requests);
        for _ in 0..expected_requests {
            let (mut stream, _) = listener.accept().expect("accept slow HTTP request");
            workers.push(thread::spawn(move || {
                thread::sleep(std::time::Duration::from_millis(180));
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                );
            }));
        }
        for worker in workers {
            worker.join().expect("slow HTTP worker completed");
        }
    });
    (format!("http://{address}"), handle)
}

fn start_fatal_http_server() -> (String, JoinHandle<bool>, Receiver<bool>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fatal HTTP server");
    listener
        .set_nonblocking(true)
        .expect("set fatal HTTP listener nonblocking");
    let address = listener.local_addr().expect("fatal HTTP server address");
    let (accepted_tx, accepted_rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        // Native executions are globally serialized across integration-test
        // processes. Give this pre-started fixture enough time to wait for the
        // execution permit; the child itself still has a 20-second watchdog.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(25);
        let (stream, _) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if std::time::Instant::now() >= deadline {
                        let _ = accepted_tx.send(false);
                        return false;
                    }
                    thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(_) => {
                    let _ = accepted_tx.send(false);
                    return false;
                }
            }
        };
        let _ = accepted_tx.send(true);
        thread::spawn(move || {
            let mut stream = stream;
            thread::sleep(std::time::Duration::from_secs(3));
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        });
        true
    });
    (format!("http://{address}"), handle, accepted_rx)
}

fn start_delayed_tcp_echo_server(expected_connections: usize) -> (u16, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind TCP echo server");
    let port = listener.local_addr().expect("TCP echo address").port();
    let handle = thread::spawn(move || {
        let mut workers = Vec::with_capacity(expected_connections);
        for _ in 0..expected_connections {
            let (mut stream, _) = listener.accept().expect("accept TCP client");
            workers.push(thread::spawn(move || {
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .expect("set TCP read timeout");
                let mut input = [0_u8; 9];
                stream.read_exact(&mut input).expect("read TCP payload");
                assert_eq!(&input, b"tcp-ping!");
                thread::sleep(std::time::Duration::from_millis(180));
                stream.write_all(b"echo-ping").expect("write TCP response");
            }));
        }
        for worker in workers {
            worker.join().expect("TCP worker completed");
        }
    });
    (port, handle)
}

fn start_invalid_byte_tcp_server(bytes: &'static [u8]) -> (u16, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind invalid-byte server");
    let port = listener.local_addr().expect("invalid-byte address").port();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept invalid-byte client");
        stream.write_all(bytes).expect("write invalid bytes");
    });
    (port, handle)
}

fn read_http_request(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 1024];
    let (header_end, content_length) = loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "request ended before headers",
            ));
        }
        request.extend_from_slice(&chunk[..read]);
        if let Some(separator) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            let header_end = separator + 4;
            let content_length = String::from_utf8_lossy(&request[..separator])
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            break (header_end, content_length);
        }
    };

    while request.len().saturating_sub(header_end) < content_length {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "request ended before body",
            ));
        }
        request.extend_from_slice(&chunk[..read]);
    }
    request.truncate(header_end + content_length);
    Ok(request)
}

/// Assert that a program prints exactly these lines.
#[track_caller]
fn assert_lines(source: &str, expected: &[&str]) {
    let output = run_native(source).unwrap_or_else(|e| panic!("compilation failed: {}", e));
    let actual: Vec<&str> = output.lines().collect();
    assert_eq!(actual, expected, "\n--- program ---\n{}", source);
}

#[track_caller]
fn assert_any_parity(source: &str, expected: &[&str]) {
    let native = run_native(source).unwrap_or_else(|e| panic!("native failed: {}", e));
    let bytecode = run_bytecode(source).unwrap_or_else(|e| panic!("bytecode VM failed: {}", e));
    assert_eq!(
        native.trim_end(),
        bytecode.trim_end(),
        "native/VM output diverged\n--- program ---\n{}",
        source
    );
    assert_eq!(
        native.lines().collect::<Vec<_>>(),
        expected,
        "\n--- program ---\n{}",
        source
    );
}

#[track_caller]
fn assert_rejected(source: &str, needle: &str) {
    match run_native(source) {
        Ok(output) => panic!("expected a compile error, but the program ran:\n{}", output),
        Err(error) => assert!(
            error.contains(needle),
            "error did not mention `{}`:\n{}",
            needle,
            error
        ),
    }
}

/// Run source through the Cranelift JIT in a resource-bounded child process.
/// Output assertions belong to the AOT helper above; JIT programs validate
/// their results in Lira and use the exit code to report success or failure.
#[track_caller]
fn assert_jit_success(source: &str) {
    let result = common::run_jit("program.li", source);
    assert_eq!(
        result,
        Ok(0),
        "JIT execution failed: {:?}\n--- program ---\n{}",
        result,
        source
    );
}

// ---------------------------------------------------------------------- //
// Scalars and control flow                                                //
// ---------------------------------------------------------------------- //

#[test]
fn arithmetic_runs_unboxed() {
    assert_lines(
        r#"
        println(2 + 3 * 4)
        println(17 / 5)
        println(17 % 5)
        println(2 ** 10)
        println(-7 / 2)
        println(1.5 + 2.25)
        println(10 / 4.0)
        "#,
        &["14", "3", "2", "1024", "-3", "3.75", "2.5"],
    );
}

#[test]
fn integer_division_by_zero_is_reported_not_a_signal() {
    let output = run_native("let a = 1\nlet b = 0\nprintln(a / b)").expect("compiles");
    assert!(
        output.contains("division by zero"),
        "unexpected output: {}",
        output
    );
}

#[test]
fn comparisons_and_short_circuit_logic() {
    assert_lines(
        r#"
        fn boom() -> bool {
            println("evaluated")
            return true
        }
        println(1 < 2)
        println(2 <= 2)
        println("abc" == "abc")
        println("abc" != "abd")
        println(false && boom())
        println(true || boom())
        "#,
        &["true", "true", "true", "true", "false", "true"],
    );
}

#[test]
fn concrete_type_checks_preserve_expression_effects() {
    assert_lines(
        r#"
        fn probe() -> int {
            println("probed")
            return 42
        }
        println(probe() is int)
        println(1 is string)
        println("text" is string)
        println([1, 2] is Array<int>)
        "#,
        &["probed", "true", "false", "true", "true"],
    );
}

#[test]
fn casts_cover_strings_in_both_directions() {
    assert_lines(
        r#"
        println(123 as string)
        println("456" as int)
        println("not a number" as int)
        println("9223372036854775808" as int)
        "#,
        &["123", "456", "0", "0"],
    );
}

// ---------------------------------------------------------------------- //
// Dynamic Any                                                            //
// ---------------------------------------------------------------------- //

#[test]
fn effect_only_conditional_tails_return_dynamic_null() {
    let source = r#"
        fn matched(value: int) {
            match value {
                1 => println("one")
                _ => println("other")
            }
        }
        fn branched(flag: bool) {
            if flag { println("yes") } else { println("no") }
        }
        let match_result = matched(1)
        let if_result = branched(false)
        println(match_result == null)
        println(if_result == null)
    "#;
    assert_any_parity(source, &["one", "no", "true", "true"]);

    assert_jit_success(
        r#"
        fn matched(value: int) {
            match value {
                1 => {}
                _ => println(1 / 0)
            }
        }
        fn branched(flag: bool) {
            if flag { println(1 / 0) } else {}
        }
        fn main() {
            if matched(1) != null { println(1 / 0) }
            if branched(false) != null { println(1 / 0) }
        }
        "#,
    );
}

#[test]
fn any_erasure_preserves_typed_nested_aggregate_aliases() {
    assert_any_parity(
        r#"
        fn first(x) { return x[0] }
        fn nested(x) { return x[1][0] }
        fn append(x) { push(x, 99) }
        fn lookup(x) { return x["answer"] }

        let numbers = [42, 7]
        let nested_numbers = [[1, 2], [3, 4]]
        let answers = { "answer": 314 }
        append(numbers)
        println(first(numbers))
        println(numbers[2])
        println(nested(nested_numbers))
        println(lookup(answers))
        "#,
        &["42", "99", "3", "314"],
    );
}

#[test]
fn dynamic_any_pop_returns_null_after_the_last_element() {
    assert_any_parity(
        r#"
        fn id(value) { return value }
        let xs = id([1])
        println(pop(xs))
        println(pop(xs))
        println(len(xs))
        "#,
        &["1", "null", "0"],
    );
    assert_jit_success(
        r#"
        fn id(value) { return value }
        fn check() -> int {
            let xs = id([1])
            let first = pop(xs)
            if first != 1 { return 1 }
            let missing = pop(xs)
            if missing != null { return 2 }
            if len(xs) != 0 { return 3 }
            return 0
        }
        fn main() {
            if check() != 0 { recv(chan(0)) }
        }
        "#,
    );
}

#[test]
fn any_typed_string_arrays_and_maps_render_and_json_stringify() {
    assert_any_parity(
        r#"
        fn at(value, index) { return value[index] }
        fn render(value) { return value as string }
        let words = ["alpha", "βeta"]
        let table = { "answer": 42 }
        println(at(words, 1))
        println(render(words))
        println(json_stringify(words))
        println(render(table))
        println(json_stringify(table))
        "#,
        &[
            "βeta",
            "[alpha, βeta]",
            r#"["alpha","βeta"]"#,
            "{answer: 42}",
            r#"{"answer":42}"#,
        ],
    );
}

#[test]
fn any_typed_struct_and_class_fields_keep_layout_metadata() {
    assert_any_parity(
        r#"
        struct Point {
            x: int
            label: string
            enabled: bool
        }
        class Counter {
            value: int
        }
        fn get_x(value) { return value.x }
        fn update_x(value) {
            value.x = 9
            return value.x
        }
        fn get_value(value) { return value.value }

        let point = Point { x: 7, label: "p", enabled: true }
        let counter = Counter { value: 11 }
        println(get_x(point))
        println(update_x(point))
        println(point.x)
        println(get_value(counter))
        println(point is Point)
        println(counter is Counter)
        "#,
        &["7", "9", "7", "11", "true", "true"],
    );
}

#[test]
fn any_enum_and_result_descriptors_do_not_fall_through_to_maps() {
    assert_any_parity(
        r#"
        enum Shape {
            Dot,
            Circle(float),
            Pair(int, string)
        }
        fn variant(value) { return value["__variant"] }
        fn data(value) { return value["__data"] }
        fn make_ok() -> Result<int, string> { return Result::Ok(7) }
        fn make_err() -> Result<int, string> { return Result::Err("bad") }

        println(variant(Shape::Dot))
        println(variant(Shape::Circle(1.5)))
        println(data(Shape::Circle(1.5)))
        println(data(Shape::Pair(2, "x")))
        println(variant(make_ok()))
        println(data(make_ok()))
        println(variant(make_err()))
        println(data(make_err()))
        "#,
        &["Dot", "Circle", "1.5", "[2, x]", "Ok", "7", "Err", "bad"],
    );
}

#[test]
fn any_optional_values_and_optional_fields_keep_presence_semantics() {
    assert_any_parity(
        r#"
        struct Maybe { value: int? }
        fn read(value) { return value.value }
        fn echo(value) { return value }
        fn some() -> int? { return 7 }
        fn none() -> int? { return null }

        println(read(Maybe { value: 7 }))
        println(read(Maybe { value: null }))
        println(echo(some()))
        println(echo(none()))
        "#,
        &["7", "null", "7", "null"],
    );
}

#[test]
fn any_nominal_object_cast_rejects_same_layout_types() {
    let output = run_native(
        r#"
        struct Left { value: int }
        struct Right { value: int }
        fn cast(value) { return value as Left }
        println(cast(Right { value: 1 }))
        "#,
    )
    .expect("nominal mismatch should compile before failing at runtime");
    assert!(
        output.contains("Any aggregate type does not match the requested type"),
        "unexpected nominal-cast diagnostic: {}",
        output
    );
}

#[test]
fn any_nominal_object_cast_rejects_struct_class_confusion() {
    let output = run_native(
        r#"
        struct Record { value: int }
        class RecordClass { value: int }
        fn cast(value) { return value as Record }
        println(cast(RecordClass { value: 1 }))
        "#,
    )
    .expect("struct/class mismatch should compile before failing at runtime");
    assert!(
        output.contains("Any aggregate type does not match the requested type"),
        "unexpected struct/class diagnostic: {}",
        output
    );
}

#[test]
fn any_mixed_conditional_and_match_returns_stay_boxed() {
    assert_any_parity(
        r#"
        fn identity(value) { return value }
        fn choose(flag) {
            return if flag as bool { identity(1) } else { identity("one") }
        }
        fn choose_match(value) {
            return match value {
                0 => identity(2),
                _ => identity("other")
            }
        }

        println(choose(true))
        println(choose(false))
        println(choose_match(0))
        println(choose_match(1))
        "#,
        &["1", "one", "2", "other"],
    );
}

#[test]
fn any_function_cast_preserves_the_callable_closure() {
    assert_any_parity(
        r#"
        fn double(value: int) -> int { return value * 2 }
        fn apply(value) {
            let callback = value as fn(int) -> int
            return callback(4)
        }
        println(apply(double))
        println(apply(|value: int| value + 3))
        "#,
        &["8", "7"],
    );
}

#[test]
fn any_channel_boundaries_use_the_channel_tag() {
    assert_any_parity(
        r#"
        fn roundtrip(channel, value) {
            send(channel, value)
            return recv(channel)
        }
        let channel = chan(1)
        println(roundtrip(channel, "ok"))
        close(channel)
        "#,
        &["ok"],
    );
}

#[test]
fn any_channel_misuse_reports_a_tag_error() {
    let output = run_native(
        r#"
        fn bad(value) { send(value, 1) }
        bad([1])
        "#,
    )
    .expect("invalid channel use should compile before failing at runtime");
    assert!(
        output.contains("expected channel, got array"),
        "unexpected channel diagnostic: {}",
        output
    );
}

#[test]
fn any_float_cast_rejects_whitespace_like_the_vm() {
    assert_any_parity(
        r#"
        fn as_float(value) { return value as float }
        println(as_float(" 2.5"))
        println(as_float("2.5"))
        "#,
        &["0", "2.5"],
    );
}

#[test]
fn any_indexing_uses_unicode_scalars() {
    assert_any_parity(
        r#"
        fn at(value, index) { return value[index] }
        println(at("hλ猫", 1))
        println(at("hλ猫", 2))
        "#,
        &["λ", "猫"],
    );
}

#[test]
fn any_numeric_comparison_keeps_large_integers_exact() {
    assert_any_parity(
        r#"
        fn greater(left, right) { return left > right }
        fn equal(left, right) { return left == right }
        println(greater(9007199254740993, 9007199254740992))
        println(equal(9007199254740993, 9007199254740992))
        println(greater(9007199254740992, 9007199254740993))
        "#,
        &["true", "false", "false"],
    );
}

#[test]
fn any_casts_and_runtime_tags_match_the_vm() {
    assert_any_parity(
        r#"
        fn as_int(value) { return value as int }
        fn as_float(value) { return value as float }
        fn as_bool(value) { return value as bool }
        fn as_string(value) { return value as string }
        fn double(value: int) -> int { return value * 2 }
        fn same_function(value) {
            let callback = value as fn(int) -> int
            return callback == callback
        }
        fn inspect(value) {
            println(value is int)
            println(value is Array<int>)
            println(value is fn(int) -> int)
        }

        println(as_int("42"))
        println(as_int("not a number"))
        println(as_int(1.9))
        println(as_float("2.5"))
        println(as_bool(""))
        println(as_string([1, 2]))
        println(same_function(double))
        inspect(7)
        inspect([1, 2])
        "#,
        &[
            "42", "0", "1", "2.5", "false", "[1, 2]", "false", "true", "false", "false", "false",
            "true", "false",
        ],
    );
}

#[test]
fn any_aggregate_function_and_channel_equality_match_the_vm() {
    assert_any_parity(
        r#"
        fn same(left, right) { return left == right }
        fn double(value: int) -> int { return value * 2 }
        fn inspect(value) {
            println(value is Map<string, int>)
            println(value is fn(int) -> int)
            println(value is Array<int>)
        }

        let values = [1]
        let table = { "answer": 1 }
        println(same(values, values))
        println(same(table, table))
        println(same(double, double))
        let channel = chan(1)
        println(same(channel, channel))
        inspect(table)
        inspect(double)
        inspect(channel)
        close(channel)
        "#,
        &[
            "false", "false", "false", "false", "true", "false", "false", "false", "true", "false",
            "false", "false", "false",
        ],
    );
}

#[test]
fn any_arithmetic_truthiness_len_and_stack_mutation_match_the_vm() {
    assert_any_parity(
        r#"
        fn arithmetic(left, right) { return (left + right) * 2 }
        fn truthy(value) { return value as bool }
        fn length(value) { return len(value) }
        fn shift(left, right) { return left << right }
        fn mutate(value) {
            push(value, 7)
            return pop(value)
        }
        println(arithmetic(2, 3))
        println(truthy(""))
        println(truthy([0]))
        println(length("λ猫"))
        println(shift(8, 64))
        println(shift(8, 65))
        println(shift(8, -1))
        println(shift(8, -4294967296))
        println(8 << 64)
        println(8 << 65)
        println(8 << -1)
        println(8 << -4294967296)
        let values = [1, 2]
        println(mutate(values))
        println(len(values))
        "#,
        &[
            "10", "false", "true", "5", "0", "0", "0", "8", "0", "0", "0", "8", "7", "2",
        ],
    );
}

#[test]
fn any_json_trees_support_index_field_len_and_mutation() {
    assert_any_parity(
        r#"
        fn item(value) { return value["items"][1] }
        fn size(value) { return len(value.items) }
        fn add_and_pop(value) {
            push(value, 3)
            return pop(value)
        }

        let tree = json_parse("{\"name\":\"lira\",\"items\":[1,2]}")
        println(item(tree))
        println(tree.name)
        println(size(tree))
        let items = tree["items"]
        println(add_and_pop(items))
        println(len(items))
        println(json_stringify(tree))
        "#,
        &[
            "2",
            "lira",
            "2",
            "3",
            "2",
            // JSON object members have canonical sorted order on both backends.
            r#"{"items":[1,2],"name":"lira"}"#,
        ],
    );
}

#[test]
fn any_cycle_rendering_is_bounded_and_identity_based() {
    let output = run_native(
        r#"
        fn render(value) { return value as string }
        var node = json_parse("[0]")
        push(node, node)
        println(render(node))
        "#,
    );
    let output = output.expect("native cycle rendering should terminate");
    assert_eq!(output.trim(), "[0, [...]]");
}

#[test]
fn any_invalid_dynamic_index_reports_a_runtime_error() {
    let output = run_native(
        r#"
        fn bad(value) { return value["wrong"] }
        println(bad([1]))
        "#,
    )
    .expect("program should compile before the dynamic error");
    assert!(
        output.contains("array index must be an integer"),
        "unexpected runtime output: {}",
        output
    );
}

#[test]
fn any_jit_executes_typed_aggregate_and_unicode_paths() {
    assert_jit_success(
        r#"
        fn first(value) { return value[0] }
        fn at(value, index) { return value[index] }
        fn main() {
            if first([17, 4]) != 17 { recv(chan(0)) }
            if at("aé", 1) != "é" { recv(chan(0)) }
            if (8 << 64) != 0 { recv(chan(0)) }
            if (8 << 65) != 0 { recv(chan(0)) }
            if (8 << -1) != 0 { recv(chan(0)) }
            if (8 << -4294967296) != 8 { recv(chan(0)) }
            if (first([8]) << 64) != 0 { recv(chan(0)) }
            if (first([8]) << 65) != 0 { recv(chan(0)) }
            if (first([8]) << -1) != 0 { recv(chan(0)) }
            if (first([8]) << -4294967296) != 8 { recv(chan(0)) }
        }
        "#,
    );
}

#[test]
fn any_registry_reclaims_high_churn_values_between_sequential_jit_runs() {
    let source = r#"
        fn churn(value) {
            let values = [value]
            push(values, value)
            pop(values)
        }
        fn main() {
            var index = 0
            while index < 5000 {
                churn(index)
                if index % 64 == 0 { collect() }
                index = index + 1
            }
            collect()
        }
        "#;
    let results = common::run_jit_sequence(&[
        ("any-churn-0.li", source),
        ("any-churn-1.li", source),
        ("any-churn-2.li", source),
    ])
    .expect("high-churn Any JIT sequence should return statuses");
    assert_eq!(results, vec![Ok(0), Ok(0), Ok(0)]);
}

#[test]
fn any_jit_runtime_failure_returns_status_and_recovers() {
    let failing = r#"
        fn bad(value) { return value["wrong"] }
        fn main() { bad([1]) }
        "#;
    let succeeding = r#"
        fn main() { println(42) }
        "#;
    let results = common::run_jit_sequence(&[
        ("any-failing.li", failing),
        ("any-succeeding.li", succeeding),
    ])
    .expect("JIT failure/recovery sequence should return statuses");
    assert_eq!(results, vec![Ok(1), Ok(0)]);
}

#[test]
fn while_loop_with_break_and_continue() {
    assert_lines(
        r#"
        var i = 0
        var total = 0
        while i < 10 {
            i = i + 1
            if i % 2 != 0 { continue }
            if i > 8 { break }
            total = total + i
        }
        println(total)
        "#,
        &["20"],
    );
}

#[test]
fn infinite_loop_exits_through_break() {
    assert_lines(
        r#"
        var n = 0
        loop {
            n = n + 1
            if n >= 5 { break }
        }
        println(n)
        "#,
        &["5"],
    );
}

#[test]
fn for_loops_iterate_arrays_and_ranges() {
    assert_lines(
        r#"
        var total = 0
        for n in [1, 2, 3, 4] { total = total + n }
        println(total)
        var counted = 0
        for i in 0..5 { counted = counted + i }
        println(counted)
        var inclusive = 0
        for i in 1..=3 { inclusive = inclusive + i }
        println(inclusive)
        "#,
        &["10", "10", "6"],
    );
}

#[test]
fn for_loops_iterate_unicode_scalars_in_native_code() {
    let source = r#"
        fn count_chars(text: string) -> int {
            var count = 0
            for ch in text { count = count + 1 }
            return count
        }
        var count = 0
        var matched = 0
        for ch in "Aé🙂" {
            count = count + 1
            if ch == 'A' { matched = matched + 1 }
            if ch == 'é' { matched = matched + 1 }
            if ch == '🙂' { matched = matched + 1 }
        }
        println(count)
        println(matched)
        println(count_chars("é🙂"))
        var nested = 0
        for outer in "ab" {
            for inner in "é🙂" {
                nested = nested + 1
                if nested > 2 { break }
            }
        }
        println(nested)
        var empty_count = 0
        for ch in "" { empty_count = empty_count + 1 }
        println(empty_count)
        "#;
    assert_any_parity(source, &["3", "3", "2", "3", "0"]);
}

#[test]
fn string_iteration_and_break_values_are_lowered_by_jit() {
    assert_jit_success(
        r#"
        fn count_chars(text: string) -> int {
            var count = 0
            for ch in text { count = count + 1 }
            return count
        }
        fn main() -> void {
            var count = 0
            var matched = 0
            for ch in "Aé🙂" {
                count = count + 1
                if ch == 'A' { matched = matched + 1 }
                if ch == 'é' { matched = matched + 1 }
                if ch == '🙂' { matched = matched + 1 }
            }
            if count != 3 { println(1 / 0) }
            if matched != 3 { println(1 / 0) }
            if count_chars("é🙂") != 2 { println(1 / 0) }

            var seen = 0
            for n in [1, 2, 3] {
                seen = seen + 1
                if n == 2 { break println("break-value") }
            }
            if seen != 2 { println(1 / 0) }
        }
        "#,
    );
}

#[test]
fn break_value_is_evaluated_once_before_leaving_a_native_loop() {
    let source = r#"
        var seen = 0
        for n in [1, 2, 3] {
            seen = seen + 1
            if n == 2 { break println("break-value") }
            println("after")
        }
        println(seen)
        "#;
    assert_any_parity(source, &["after", "break-value", "2"]);
}

#[test]
fn recursion_and_mutual_recursion() {
    assert_lines(
        r#"
        fn is_even(n: int) -> bool {
            if n == 0 { return true }
            return is_odd(n - 1)
        }
        fn is_odd(n: int) -> bool {
            if n == 0 { return false }
            return is_even(n - 1)
        }
        fn fib(n: int) -> int {
            if n < 2 { return n }
            return fib(n - 1) + fib(n - 2)
        }
        println(fib(20))
        println(is_even(10))
        println(is_odd(10))
        "#,
        &["6765", "true", "false"],
    );
}

#[test]
fn main_is_invoked_once_even_when_the_top_level_calls_it() {
    assert_lines("fn main() { println(\"once\") }\nmain()", &["once"]);
    assert_lines("fn main() { println(\"auto\") }", &["auto"]);
}

// ---------------------------------------------------------------------- //
// Strings                                                                 //
// ---------------------------------------------------------------------- //

#[test]
fn string_concatenation_stringifies_the_other_operand() {
    assert_lines(
        r#"
        let n = 42
        println("n = " + n)
        println("f = " + 1.5)
        println("b = " + true)
        println("interpolated: ${n + 1}")
        println(len("hello"))
        "#,
        &["n = 42", "f = 1.5", "b = true", "interpolated: 43", "5"],
    );
}

#[test]
fn string_indexing_returns_one_unicode_scalar_as_a_string() {
    assert_lines(
        r#"
        let text = "héλ猫"
        println(text[0])
        println(text[1])
        println(text[2])
        println(text[3])
        println("[" + text[3] + "]")
        println(text[1] == "é")
        "#,
        &["h", "é", "λ", "猫", "[猫]", "true"],
    );
}

#[test]
fn string_indexing_reports_negative_and_past_end_indices() {
    for source in ["println(\"hi\"[-1])", "println(\"hi\"[2])"] {
        let output = run_native(source).expect("compiles");
        assert!(
            output.contains("out of bounds"),
            "unexpected output for `{source}`: {output}"
        );
    }
}

// ---------------------------------------------------------------------- //
// Structs                                                                 //
// ---------------------------------------------------------------------- //

#[test]
fn struct_fields_load_at_constant_offsets() {
    assert_lines(
        r#"
        struct Point {
            x: int
            y: int

            fn sum(self) -> int { return self.x + self.y }
        }
        struct Line { start: Point, end: Point }

        let p = Point { x: 10, y: 20 }
        println(p.x)
        println(p.y)
        println(p.sum())
        let l = Line { start: Point { x: 1, y: 2 }, end: Point { x: 3, y: 4 } }
        println(l.end.y)
        "#,
        &["10", "20", "30", "4"],
    );
}

#[test]
fn narrow_struct_fields_round_trip_through_memory() {
    assert_lines(
        r#"
        struct Packed {
            a: int8
            b: int32
            c: bool
            d: float
        }
        let p = Packed { a: -5, b: 100000, c: true, d: 0.5 }
        println(p.a)
        println(p.b)
        println(p.c)
        println(p.d)
        "#,
        &["-5", "100000", "true", "0.5"],
    );
}

#[test]
fn struct_fields_are_mutable_in_place() {
    assert_lines(
        r#"
        struct Counter { value: int }
        let c = Counter { value: 1 }
        c.value = c.value + 41
        println(c.value)
        "#,
        &["42"],
    );
}

#[test]
fn impl_blocks_provide_static_and_instance_methods() {
    assert_lines(
        r#"
        struct Counter { value: int }
        impl Counter {
            fn new() -> Counter { return Counter { value: 0 } }
            fn get(self) -> int { return self.value }
            fn bump(self) -> Counter { return Counter { value: self.value + 1 } }
        }
        let a = Counter.new()
        println(a.get())
        println(a.bump().bump().get())
        "#,
        &["0", "2"],
    );
}

// ---------------------------------------------------------------------- //
// Enums and pattern matching                                              //
// ---------------------------------------------------------------------- //

#[test]
fn enum_payloads_survive_a_round_trip() {
    assert_lines(
        r#"
        enum Shape {
            Dot,
            Circle(float),
            Rect(int, int)
        }
        fn describe(s: Shape) -> string {
            return match s {
                Shape::Dot => "dot",
                Shape::Circle(r) => "circle " + r,
                Shape::Rect(w, h) => "rect " + (w * h)
            }
        }
        println(describe(Shape::Dot))
        println(describe(Shape::Circle(1.5)))
        println(describe(Shape::Rect(3, 4)))
        "#,
        &["dot", "circle 1.5", "rect 12"],
    );
}

#[test]
fn match_supports_literals_ranges_guards_and_bindings() {
    assert_lines(
        r#"
        fn classify(n: int) -> string {
            return match n {
                0 => "zero",
                1..5 => "small",
                5..=9 => "medium",
                x if x < 0 => "negative",
                other => "large:" + other
            }
        }
        println(classify(0))
        println(classify(3))
        println(classify(7))
        println(classify(-2))
        println(classify(99))
        "#,
        &["zero", "small", "medium", "negative", "large:99"],
    );
}

#[test]
fn struct_patterns_bind_fields() {
    assert_lines(
        r#"
        struct Point { x: int, y: int }
        fn area(p: Point) -> int {
            return match p {
                Point { x, y } => x * y
            }
        }
        println(area(Point { x: 6, y: 7 }))
        "#,
        &["42"],
    );
}

#[test]
fn struct_patterns_use_nested_field_types() {
    assert_lines(
        r#"
        struct Point { x: int, label: string }
        struct Envelope { point: Point, suffix: string }
        fn describe(envelope: Envelope) -> string {
            return match envelope {
                Envelope { point: Point { x, label }, suffix } =>
                    label + ":" + x + ":" + suffix
            }
        }
        println(describe(Envelope {
            point: Point { x: 6, label: "point" },
            suffix: "done"
        }))
        "#,
        &["point:6:done"],
    );
}

#[test]
fn struct_patterns_bind_fields_in_jit() {
    assert_jit_success(
        r#"
        struct Point { x: int, y: int }
        fn area(point: Point) -> int {
            return match point {
                Point { x, y } => x * y
            }
        }
        println(area(Point { x: 6, y: 7 }))
        "#,
    );
}

#[test]
fn struct_patterns_reject_unknown_fields() {
    assert_rejected(
        r#"
        struct Point { x: int }
        fn read(point: Point) -> int {
            return match point {
                Point { missing } => missing
            }
        }
        println(read(Point { x: 1 }))
        "#,
        "has no field 'missing'",
    );
}

#[test]
fn enum_reflection_reports_the_variant_name() {
    assert_lines(
        r#"
        enum Color { Red, Green, Blue }
        let c = Color::Green
        println(c.__enum)
        println(c.__variant)
        println(Color::Blue.__variant)
        "#,
        &["Color", "Green", "Blue"],
    );
}

// ---------------------------------------------------------------------- //
// Arrays                                                                  //
// ---------------------------------------------------------------------- //

#[test]
fn arrays_index_push_and_pop() {
    assert_lines(
        r#"
        let xs = [10, 20, 30]
        println(xs[0])
        println(len(xs))
        push(xs, 40)
        println(xs[3])
        xs[1] = 99
        println(xs[1])
        println(pop(xs))
        println(len(xs))
        "#,
        &["10", "3", "40", "99", "40", "3"],
    );
}

#[test]
fn typed_array_pop_returns_optional_for_values_and_references() {
    let source = r#"
        struct Point { x: int }
        class Box { value: int }

        fn pop_point_x(xs: [Point]) -> int {
            var point = pop(xs)?
            point.x = 99
            return point.x
        }

        fn pop_box_value() -> int {
            let original_box = Box { value: 8 }
            let boxes = [original_box]
            let boxed = pop(boxes)?
            boxed.value = 99
            return original_box.value
        }

        let numbers: [int] = [7]
        println(pop(numbers))
        println(len(numbers))
        println(pop(numbers))

        let original = Point { x: 42 }
        let points: [Point] = [original]
        println(pop_point_x(points))
        println(original.x)

        println(pop_box_value())
    "#;
    assert_any_parity(source, &["7", "0", "null", "99", "42", "99"]);
    assert_lines(
        r#"
        let words: [string] = ["word"]
        println(words.pop())
        println(words.pop())
        "#,
        &["word", "null"],
    );
    assert_jit_success(
        r#"
        fn check() -> int {
            let numbers: [int] = [7]
            let first = numbers.pop()
            if first == null { return 1 }
            let value = first ?? 0
            if value != 7 { return 2 }
            if len(numbers) != 0 { return 3 }
            let missing = pop(numbers)
            if missing != null { return 4 }
            return 0
        }
        fn main() {
            if check() != 0 { recv(chan(0)) }
        }
        "#,
    );
}

#[test]
fn empty_arrays_are_refined_by_push_or_remain_safely_empty() {
    assert_lines(
        r#"
        let stack = []
        push(stack, 10)
        push(stack, 20)
        println(pop(stack))
        println(pop(stack))

        let empty = []
        for value in empty {
            println(value)
        }
        println("still empty")
        "#,
        &["20", "10", "still empty"],
    );
}

#[test]
fn out_of_bounds_indexing_is_reported() {
    let output = run_native("let xs = [1, 2]\nprintln(xs[5])").expect("compiles");
    assert!(
        output.contains("out of bounds"),
        "unexpected output: {}",
        output
    );
}

#[test]
fn arrays_of_floats_survive_the_uniform_slot_representation() {
    assert_lines(
        r#"
        let xs = [1.5, 2.5, 3.0]
        var total = 0.0
        for x in xs { total = total + x }
        println(total)
        println(xs[2])
        "#,
        &["7", "3"],
    );
}

// ---------------------------------------------------------------------- //
// Fibers and channels                                                     //
// ---------------------------------------------------------------------- //

#[test]
fn spawned_fibers_interleave_at_yield_points() {
    assert_lines(
        r#"
        fn ticker(name: string, rounds: int) {
            var i = 0
            while i < rounds {
                println(name + i)
                fiber_yield()
                i = i + 1
            }
        }
        fn main() {
            spawn ticker("a", 2)
            spawn ticker("b", 2)
        }
        "#,
        &["a0", "b0", "a1", "b1"],
    );
}

#[test]
fn an_unbuffered_channel_is_a_rendezvous() {
    assert_lines(
        r#"
        fn producer(ch, count: int) {
            var i = 0
            while i < count {
                send(ch, i * 10)
                i = i + 1
            }
        }
        fn main() {
            let ch = chan(0)
            spawn producer(ch, 3)
            var n = 0
            while n < 3 {
                println(recv(ch))
                n = n + 1
            }
        }
        "#,
        &["0", "10", "20"],
    );
}

#[test]
fn a_buffered_channel_lets_the_sender_run_ahead() {
    assert_lines(
        r#"
        fn producer(ch: Channel<int>) {
            send(ch, 1)
            send(ch, 2)
            println("sent both")
        }
        fn main() {
            let ch = chan(4)
            spawn producer(ch)
            fiber_yield()
            println(recv(ch))
            println(recv(ch))
        }
        "#,
        &["sent both", "1", "2"],
    );
}

#[test]
fn typed_string_channel_round_trips_through_native_slots() {
    assert_lines(
        r#"
        fn produce(ch: Channel<string>) -> void {
            send(ch, "hello from a fiber")
        }
        fn main() {
            let ch: Channel<string> = chan(1)
            spawn produce(ch)
            println(recv(ch))
        }
        "#,
        &["hello from a fiber"],
    );
}

#[test]
fn typed_channel_lowering_runs_through_the_jit_entry_point() {
    assert_jit_success(
        r#"
        fn main() -> void {
            let ch: Channel<string> = chan(1)
            send(ch, "jit")
            let value: string = recv(ch)
            if value != "jit" { close(ch) }
        }
        "#,
    );
}

#[test]
fn a_blocked_program_reports_a_deadlock_instead_of_hanging() {
    let output = run_native(
        r#"
        fn main() {
            let ch: Channel<int> = chan(0)
            println("waiting")
            recv(ch)
            println("unreachable")
        }
        "#,
    )
    .expect("compiles");
    assert!(output.contains("waiting"), "unexpected output: {}", output);
    assert!(output.contains("deadlock"), "unexpected output: {}", output);
    assert!(
        !output.contains("unreachable"),
        "the blocked fiber should never have resumed: {}",
        output
    );
}

#[test]
fn native_sleep_parks_only_the_calling_fibers() {
    let source = r#"
        fn sleeper(ch: Channel<int>) {
            sleep(200)
            send(ch, 1)
        }
        fn main() {
            let ch: Channel<int> = chan(8)
            spawn sleeper(ch)
            spawn sleeper(ch)
            spawn sleeper(ch)
            spawn sleeper(ch)
            spawn sleeper(ch)
            spawn sleeper(ch)
            spawn sleeper(ch)
            spawn sleeper(ch)
            var total = 0
            for i in 0..8 { total = total + recv(ch) }
            println(total)
        }
    "#;
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("native sleep scratch directory");
    let source_path = dir.join("program.li");
    std::fs::write(&source_path, source).expect("native sleep source");
    let output = common::run_aot(&source_path, source).expect("run native sleep binary");
    let elapsed = output.elapsed;
    let mut text = output.stdout_text();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(text.lines().collect::<Vec<_>>(), vec!["8"]);
    assert!(
        elapsed < std::time::Duration::from_millis(1_100),
        "eight 200ms sleeps exceeded the 4-worker overlap bound: {elapsed:?}"
    );
}

#[test]
fn jit_sleep_parks_only_the_calling_fibers() {
    let source = r#"
        fn sleeper(ch: Channel<int>) {
            sleep(200)
            send(ch, 1)
        }
        fn main() {
            let ch: Channel<int> = chan(8)
            spawn sleeper(ch)
            spawn sleeper(ch)
            spawn sleeper(ch)
            spawn sleeper(ch)
            spawn sleeper(ch)
            spawn sleeper(ch)
            spawn sleeper(ch)
            spawn sleeper(ch)
            var total = 0
            for i in 0..8 { total = total + recv(ch) }
            if total != 8 { println(1 / 0) }
        }
    "#;
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join("sleep.li");
    std::fs::write(&path, source).expect("write sleep source");
    let timed = common::run_jit_timed(path.to_str().expect("utf-8 path"), source);
    let elapsed = timed
        .as_ref()
        .map(|(_, elapsed)| *elapsed)
        .unwrap_or(std::time::Duration::MAX);
    let result = timed.map(|(status, _)| status);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(result, Ok(0), "JIT sleep source failed: {result:?}");
    assert!(
        elapsed < std::time::Duration::from_millis(1_100),
        "eight 200ms sleeps exceeded the 4-worker overlap bound: {elapsed:?}"
    );
}

#[test]
fn sequential_jit_runs_reset_fiber_ids_and_deadlock_state() {
    let deadlock = r#"
        fn main() {
            recv(chan(0))
        }
        "#;
    let identity = r#"
        fn child() {
            if fiber_id() != 1 { recv(chan(0)) }
        }
        fn main() { spawn child() }
        "#;
    let results = common::run_jit_sequence(&[
        ("deadlock.li", deadlock),
        ("identity.li", identity),
        ("identity-again.li", identity),
    ])
    .expect("sequential fiber-state JIT runs should return statuses");
    assert_eq!(results, vec![Ok(1), Ok(0), Ok(0)]);
}

#[test]
fn sequential_jit_runtime_error_returns_and_next_run_succeeds() {
    let failing = "fn main() { println(1 / 0) }";
    let succeeding = "fn main() { println(42) }";
    let results =
        common::run_jit_sequence(&[("failing.li", failing), ("succeeding.li", succeeding)])
            .expect("sequential runtime-error JIT runs should return statuses");
    assert_eq!(results, vec![Ok(1), Ok(0)]);
}

#[test]
fn jit_runtime_error_cancels_long_sleep_before_joining_workers() {
    let failing = r#"
        fn sleeper() { sleep(30_000) }
        fn main() {
            spawn sleeper()
            println(1 / 0)
        }
    "#;
    let (results, elapsed) = common::run_jit_sequence_timed(&[
        ("long-sleep-failing.li", failing),
        ("after-failure.li", "fn main() { println(7) }"),
    ])
    .expect("runtime failure/recovery JIT sequence should return statuses");
    assert_eq!(results, vec![Ok(1), Ok(0)]);
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "runtime failure waited for an outstanding long sleep: {:?}",
        elapsed
    );
}

#[test]
fn jit_runtime_error_orphans_an_outstanding_http_worker() {
    let (base, server, accepted) = start_fatal_http_server();
    let source = format!(
        r#"
        fn fetch(url: string) {{
            let (status, body) = http_get(url)
        }}
        fn main() {{
            spawn fetch("{base}")
            sleep(100)
            println(1 / 0)
        }}
        "#
    );
    let (results, elapsed) = common::run_jit_sequence_timed(&[
        ("slow-http-failing.li", &source),
        ("after-http-failure.li", "fn main() { println(8) }"),
    ])
    .expect("HTTP runtime failure/recovery JIT sequence should return statuses");
    assert_eq!(results, vec![Ok(1), Ok(0)]);
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "runtime failure waited for the outstanding HTTP worker: {elapsed:?}"
    );
    assert!(accepted
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("fatal HTTP accept signal"));
    assert!(server.join().expect("fatal HTTP server completed"));
}

#[test]
fn aot_runtime_error_orphans_an_outstanding_http_worker_without_waiting() {
    let (base, server, accepted) = start_fatal_http_server();
    let source = format!(
        r#"
        fn fetch(url: string) {{
            let (status, body) = http_get(url)
        }}
        fn main() {{
            spawn fetch("{base}")
            sleep(100)
            println(1 / 0)
        }}
        "#
    );
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("AOT runtime failure scratch directory");
    let source_path = dir.join("program.li");
    std::fs::write(&source_path, &source).expect("AOT runtime failure source");
    let output = common::run_aot(&source_path, &source).expect("AOT runtime failure should run");
    let elapsed = output.elapsed;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("division by zero"));
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "AOT runtime failure waited for the HTTP worker: {:?}",
        elapsed
    );
    let _ = std::fs::remove_dir_all(&dir);
    assert!(accepted
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("AOT fatal HTTP accept signal"));
    assert!(server.join().expect("AOT fatal HTTP server completed"));
}

#[test]
fn repeated_fatal_file_runs_reclaim_orphaned_handles_before_table_exhaustion() {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let source = |path: &str| {
        format!(
            r#"
            fn opener(path: string) {{
                let handle = file_open(path, 1)
                fiber_yield()
            }}
            fn main() {{
                spawn opener("{path}")
                fiber_yield()
                println(1 / 0)
            }}
            "#
        )
    };
    let mut programs = Vec::with_capacity(41);
    for index in 0..40 {
        let path = dir.join(format!("fatal-{index}.tmp"));
        let source = source(path.to_str().expect("UTF-8 temporary path"));
        let program = dir.join(format!("fatal-{index}.li"));
        std::fs::write(&program, &source).expect("write fatal source");
        programs.push((format!("fatal-{index}.li"), source));
    }

    let success = r#"
        fn main() {
            let path = env_temp_dir() + "/lira-orphan-recovery.txt"
            let handle = file_open(path, 1)
            let written = file_write(handle, "recovered")
            file_close(handle)
            let reader = file_open(path, 0)
            let body = file_read(reader, 32)
            file_close(reader)
            remove(path)
            if written != 9 || body != "recovered" { println(1 / 0) }
        }
    "#;
    let success_path = dir.join("recovered.li");
    std::fs::write(&success_path, success).expect("write recovery source");
    programs.push(("recovered.li".to_owned(), success.to_owned()));
    let references = programs
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect::<Vec<_>>();
    let results = common::run_jit_sequence(&references)
        .expect("repeated fatal-file JIT sequence should return statuses");
    assert_eq!(results.len(), 41);
    for (index, result) in results.iter().take(40).enumerate() {
        assert_eq!(result, &Ok(1), "fatal run {index} did not return status 1");
    }
    assert_eq!(results[40], Ok(0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn io_queue_exhaustion_returns_runtime_failure_without_double_free() {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let mut spawns = String::new();
    for _ in 0..140 {
        spawns.push_str("            spawn sleeper()\n");
    }
    let source = format!(
        r#"
        fn sleeper() {{ sleep(30_000) }}
        fn main() {{
{spawns}            fiber_yield()
        }}
        "#
    );
    let success = "fn main() { println(11) }";
    let results = common::run_jit_sequence(&[
        ("queue-exhaustion.li", &source),
        ("after-queue-exhaustion.li", success),
    ])
    .expect("queue exhaustion/recovery JIT sequence should return statuses");
    assert_eq!(results, vec![Ok(1), Ok(0)]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fibers_get_their_own_stacks() {
    // Deep recursion inside a spawned fiber only works if the fiber really is
    // running on its own stack rather than borrowing the scheduler's frame.
    assert_lines(
        r#"
        fn depth(n: int) -> int {
            if n == 0 { return 0 }
            return 1 + depth(n - 1)
        }
        fn worker(ch) {
            send(ch, depth(1000))
        }
        fn main() {
            let ch = chan(1)
            spawn worker(ch)
            println(recv(ch))
        }
        "#,
        &["1000"],
    );
}

// ---------------------------------------------------------------------- //
// Built-ins                                                               //
// ---------------------------------------------------------------------- //

#[test]
fn http_builtins_round_trip_typed_results_through_aot() {
    let (base, server) = start_http_server(3);
    let source = r#"
        fn show(response: (int, string)) -> void {
            let (status, body) = response
            println(status)
            println(body)
        }

        let get_response = http_get("__BASE__/get")
        show(get_response)
        let post_response = http_post("__BASE__/post", "payload", "text/plain")
        show(post_response)
        let request_response = http_request(
            "PATCH",
            "__BASE__/request",
            "X-Test: yes\nnot a header\n: invalid",
            "custom body"
        )
        show(request_response)
        "#
    .replace("__BASE__", &base);

    let output = run_native(&source).expect("native HTTP source compiles and runs");
    assert_eq!(
        output.lines().collect::<Vec<_>>(),
        ["404", "missing", "201", "created", "202", "patched"]
    );

    let requests = server.join().expect("HTTP test server completed");
    assert_eq!(requests.len(), 3);
    let post = String::from_utf8_lossy(&requests[1]);
    assert!(post
        .to_ascii_lowercase()
        .contains("content-type: text/plain"));
    assert!(
        post.ends_with("payload"),
        "POST body was not complete: {post:?}"
    );
    let custom = String::from_utf8_lossy(&requests[2]);
    assert!(custom.to_ascii_lowercase().contains("x-test: yes"));
    assert!(
        custom.ends_with("custom body"),
        "custom body was not complete: {custom:?}"
    );
    assert!(!custom.to_ascii_lowercase().contains("not a header"));
}

#[test]
fn http_worker_result_alloc_failure_returns_typed_error_in_aot_and_jit() {
    let (base, server) = start_http_server(1);
    let source = format!(
        r#"
        env_set("LIRA_TEST_FAIL_HTTP_RESULT_ALLOC", "1")
        let (status, body) = http_get("{base}/get")
        env_remove("LIRA_TEST_FAIL_HTTP_RESULT_ALLOC")
        println(status)
        println(body)
        "#
    );
    assert_lines(&source, &["-1", "HTTP worker failed"]);
    server
        .join()
        .expect("AOT HTTP allocation-failure server completed");

    let (base, server) = start_http_server(1);
    let jit_source = format!(
        r#"
        fn main() {{
            env_set("LIRA_TEST_FAIL_HTTP_RESULT_ALLOC", "1")
            let (status, body) = http_get("{base}/get")
            env_remove("LIRA_TEST_FAIL_HTTP_RESULT_ALLOC")
            if status != -1 {{ println(1 / 0) }}
            if body != "HTTP worker failed" {{ println(1 / 0) }}
        }}
        "#
    );
    assert_jit_success(&jit_source);
    server
        .join()
        .expect("JIT HTTP allocation-failure server completed");
}

#[test]
fn http_oversized_and_empty_bodies_preserve_status_in_all_backends() {
    let oversized = 10 * 1024 * 1024 + 1;
    let (base, server) = start_http_body_server(413, oversized);
    let source = format!(
        r#"
        let (status, body) = http_get("{base}")
        println(status)
        println(body == "")
        "#
    );
    assert_lines(&source, &["413", "true"]);
    server.join().expect("AOT oversized HTTP server completed");

    let (base, server) = start_http_body_server(413, oversized);
    let vm_source = format!(
        r#"
        let (status, body) = http_get("{base}")
        println(status)
        println(body == "")
        "#
    );
    assert_eq!(
        run_bytecode(&vm_source)
            .expect("VM oversized HTTP source ran")
            .lines()
            .collect::<Vec<_>>(),
        ["413", "true"]
    );
    server.join().expect("VM oversized HTTP server completed");

    let (base, server) = start_http_body_server(413, oversized);
    let jit_source = format!(
        r#"
        fn main() {{
            let (status, body) = http_get("{base}")
            if status != 413 {{ println(1 / 0) }}
            if body != "" {{ println(1 / 0) }}
        }}
        "#
    );
    assert_jit_success(&jit_source);
    server.join().expect("JIT oversized HTTP server completed");

    let (base, server) = start_http_body_server(204, 0);
    let empty_source = format!(
        r#"
        let (status, body) = http_get("{base}")
        println(status)
        println(body == "")
        "#
    );
    assert_lines(&empty_source, &["204", "true"]);
    server.join().expect("AOT empty HTTP server completed");

    let (base, server) = start_http_body_server(204, 0);
    let vm_empty_source = format!(
        r#"
        let (status, body) = http_get("{base}")
        println(status)
        println(body == "")
        "#
    );
    assert_eq!(
        run_bytecode(&vm_empty_source)
            .expect("VM empty HTTP source ran")
            .lines()
            .collect::<Vec<_>>(),
        ["204", "true"]
    );
    server.join().expect("VM empty HTTP server completed");

    let (base, server) = start_http_body_server(204, 0);
    let jit_empty_source = format!(
        r#"
        fn main() {{
            let (status, body) = http_get("{base}")
            if status != 204 {{ println(1 / 0) }}
            if body != "" {{ println(1 / 0) }}
        }}
        "#
    );
    assert_jit_success(&jit_empty_source);
    server.join().expect("JIT empty HTTP server completed");
}

#[test]
fn slow_http_requests_overlap_in_aot_workers() {
    let (base, server) = start_slow_http_server(8);
    let source = format!(
        r#"
        fn fetch(url: string, ch: Channel<int>) {{
            let (status, body) = http_get(url)
            send(ch, status)
        }}
        fn main() {{
            let ch: Channel<int> = chan(8)
            spawn fetch("{base}", ch)
            spawn fetch("{base}", ch)
            spawn fetch("{base}", ch)
            spawn fetch("{base}", ch)
            spawn fetch("{base}", ch)
            spawn fetch("{base}", ch)
            spawn fetch("{base}", ch)
            spawn fetch("{base}", ch)
            var total = 0
            for i in 0..8 {{ total = total + recv(ch) }}
            println(total)
        }}
        "#
    );
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("slow HTTP AOT scratch directory");
    let source_path = dir.join("program.li");
    std::fs::write(&source_path, &source).expect("slow HTTP AOT source");
    let output = common::run_aot(&source_path, &source).expect("run slow HTTP AOT binary");
    let elapsed = output.elapsed;
    let _ = std::fs::remove_dir_all(&dir);
    server.join().expect("slow HTTP server completed");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .collect::<Vec<_>>(),
        ["1600"]
    );
    assert!(
        elapsed < std::time::Duration::from_millis(1_100),
        "eight delayed HTTP requests exceeded the 4-worker overlap bound: {elapsed:?}"
    );
}

#[test]
fn slow_http_requests_overlap_in_jit_workers() {
    let (base, server) = start_slow_http_server(8);
    let source = format!(
        r#"
        fn fetch(url: string, ch: Channel<int>) {{
            let (status, body) = http_get(url)
            send(ch, status)
        }}
        fn main() {{
            let ch: Channel<int> = chan(8)
            spawn fetch("{base}", ch)
            spawn fetch("{base}", ch)
            spawn fetch("{base}", ch)
            spawn fetch("{base}", ch)
            spawn fetch("{base}", ch)
            spawn fetch("{base}", ch)
            spawn fetch("{base}", ch)
            spawn fetch("{base}", ch)
            var total = 0
            for i in 0..8 {{ total = total + recv(ch) }}
            if total != 1600 {{ println(1 / 0) }}
        }}
        "#
    );
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join("slow-http.li");
    std::fs::write(&path, &source).expect("write slow HTTP source");
    let timed = common::run_jit_timed(path.to_str().expect("utf-8 path"), &source);
    let elapsed = timed
        .as_ref()
        .map(|(_, elapsed)| *elapsed)
        .unwrap_or(std::time::Duration::MAX);
    let result = timed.map(|(status, _)| status);
    let _ = std::fs::remove_dir_all(&dir);
    server.join().expect("slow HTTP server completed");
    assert_eq!(result, Ok(0), "slow HTTP JIT failed: {result:?}");
    assert!(
        elapsed < std::time::Duration::from_millis(1_100),
        "eight delayed HTTP requests exceeded the 4-worker overlap bound: {elapsed:?}"
    );
}

#[test]
fn delayed_tcp_requests_overlap_in_aot_workers() {
    let (port, server) = start_delayed_tcp_echo_server(8);
    let source = format!(
        r#"
        fn talk(host: string, port: int, ch: Channel<int>) {{
            let handle = tcp_connect(host, port)
            if handle < 0 {{
                send(ch, 0)
            }} else {{
                let wrote = tcp_write(handle, "tcp-ping!")
                let body = tcp_read(handle, 32)
                let closed = tcp_close(handle)
                if wrote == 9 && body == "echo-ping" && closed {{
                    send(ch, 1)
                }} else {{
                    send(ch, 0)
                }}
            }}
        }}
        fn main() {{
            let ch: Channel<int> = chan(8)
            spawn talk("127.0.0.1", {port}, ch)
            spawn talk("127.0.0.1", {port}, ch)
            spawn talk("127.0.0.1", {port}, ch)
            spawn talk("127.0.0.1", {port}, ch)
            spawn talk("127.0.0.1", {port}, ch)
            spawn talk("127.0.0.1", {port}, ch)
            spawn talk("127.0.0.1", {port}, ch)
            spawn talk("127.0.0.1", {port}, ch)
            var total = 0
            for i in 0..8 {{ total = total + recv(ch) }}
            println(total)
        }}
        "#
    );
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("TCP AOT scratch directory");
    let source_path = dir.join("program.li");
    std::fs::write(&source_path, &source).expect("TCP AOT source");
    let output = common::run_aot(&source_path, &source).expect("run TCP AOT binary");
    let elapsed = output.elapsed;
    let _ = std::fs::remove_dir_all(&dir);
    server.join().expect("TCP server completed");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .collect::<Vec<_>>(),
        ["8"]
    );
    assert!(
        elapsed < std::time::Duration::from_millis(1_100),
        "eight delayed TCP requests exceeded the 4-worker overlap bound: {elapsed:?}"
    );
}

#[test]
fn delayed_tcp_requests_overlap_in_jit_workers() {
    let (port, server) = start_delayed_tcp_echo_server(8);
    let source = format!(
        r#"
        fn talk(host: string, port: int, ch: Channel<int>) {{
            let handle = tcp_connect(host, port)
            if handle < 0 {{
                send(ch, 0)
            }} else {{
                let wrote = tcp_write(handle, "tcp-ping!")
                let body = tcp_read(handle, 32)
                let closed = tcp_close(handle)
                if wrote == 9 && body == "echo-ping" && closed {{
                    send(ch, 1)
                }} else {{
                    send(ch, 0)
                }}
            }}
        }}
        fn main() {{
            let ch: Channel<int> = chan(8)
            spawn talk("127.0.0.1", {port}, ch)
            spawn talk("127.0.0.1", {port}, ch)
            spawn talk("127.0.0.1", {port}, ch)
            spawn talk("127.0.0.1", {port}, ch)
            spawn talk("127.0.0.1", {port}, ch)
            spawn talk("127.0.0.1", {port}, ch)
            spawn talk("127.0.0.1", {port}, ch)
            spawn talk("127.0.0.1", {port}, ch)
            var total = 0
            for i in 0..8 {{ total = total + recv(ch) }}
            if total != 8 {{ println(1 / 0) }}
        }}
        "#
    );
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join("tcp.li");
    std::fs::write(&path, &source).expect("write TCP source");
    let timed = common::run_jit_timed(path.to_str().expect("utf-8 path"), &source);
    let elapsed = timed
        .as_ref()
        .map(|(_, elapsed)| *elapsed)
        .unwrap_or(std::time::Duration::MAX);
    let result = timed.map(|(status, _)| status);
    let _ = std::fs::remove_dir_all(&dir);
    server.join().expect("TCP server completed");
    assert_eq!(result, Ok(0), "TCP JIT source failed: {result:?}");
    assert!(
        elapsed < std::time::Duration::from_millis(1_100),
        "eight delayed TCP requests exceeded the 4-worker overlap bound: {elapsed:?}"
    );
}

#[test]
fn dns_lookup_reports_localhost_and_unknown_host_in_aot_and_jit() {
    let source = r#"
        let localhost = dns_lookup("localhost")
        let unknown = dns_lookup("!")
        println(localhost != "")
        println(unknown == "")
    "#;
    assert_lines(source, &["true", "true"]);

    let jit_source = r#"
        fn main() {
            let localhost = dns_lookup("localhost")
            let unknown = dns_lookup("!")
            if localhost == "" || unknown != "" { println(1 / 0) }
        }
    "#;
    assert_jit_success(jit_source);
}

#[test]
fn http_transport_failure_returns_typed_negative_status_and_message() {
    let closed_url = {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind closed-port probe");
        format!(
            "http://{}",
            listener.local_addr().expect("closed-port address")
        )
    };
    let source = format!(
        "let response = http_get(\"{closed_url}\")\nlet (status, body) = response\nprintln(status)\nprintln(body)\n"
    );
    let output = run_native(&source).expect("native HTTP failure source compiles and runs");
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.first(), Some(&"-1"));
    assert_eq!(lines.len(), 2);
    assert!(
        lines[1].starts_with("HTTP error:") && lines[1].len() > "HTTP error:".len(),
        "transport failure should preserve a descriptive message: {:?}",
        lines[1]
    );
}

#[test]
fn http_invalid_argument_type_is_rejected_before_native_codegen() {
    assert_rejected("let response = http_get(42)", "Argument type mismatch");
}

#[test]
fn http_builtins_are_validated_inside_the_jit() {
    let (base, server) = start_http_server(3);
    let source = r#"
        fn require(ok: bool) -> void {
            if !ok {
                let zero = 0
                println(1 / zero)
            }
        }
        let get_response = http_get("__BASE__/get")
        let (get_status, get_body) = get_response
        require(get_status == 404 && get_body == "missing")
        let post_response = http_post("__BASE__/post", "payload", "text/plain")
        let (post_status, post_body) = post_response
        require(post_status == 201 && post_body == "created")
        let request_response = http_request(
            "PATCH",
            "__BASE__/request",
            "X-Test: yes\nnot a header\n: invalid",
            "custom body"
        )
        let (request_status, request_body) = request_response
        require(request_status == 202 && request_body == "patched")
        "#
    .replace("__BASE__", &base);
    assert_jit_success(&source);
    let requests = server.join().expect("JIT HTTP test server completed");
    assert_eq!(requests.len(), 3);
}

#[test]
fn math_builtins_lower_to_instructions_and_libm() {
    assert_lines(
        r#"
        println(sqrt(16.0))
        println(floor(2.7))
        println(ceil(2.1))
        println(trunc(-2.7))
        println(round(2.5))
        println(pow(2.0, 10.0))
        println(is_nan(0.0))
        println(is_finite(1.0))
        println(abs(-3.5))
        "#,
        &["4", "2", "3", "-2", "3", "1024", "false", "true", "3.5"],
    );
}

#[test]
fn string_builtins_index_by_character_not_byte() {
    assert_lines(
        r#"
        println(str_to_upper("hello"))
        println(str_to_lower("HELLO"))
        println(str_substring("hello world", 0, 5))
        println(str_index_of("hello world", "world"))
        println(str_trim("  padded  "))
        println(len(str_split("a,b,c", ",")))
        println(str_split("a,b,c", ",")[1])
        println(str_char_code("abc", 1))
        println(str_from_char_code(65))
        "#,
        &[
            "HELLO", "hello", "hello", "6", "padded", "3", "b", "98", "A",
        ],
    );
}

#[test]
fn regex_match_find_and_validation_follow_vm_results() {
    assert_lines(
        r#"
        println(regex_match("^hello$", "hello"))
        println(regex_match("^hello$", "hello world"))
        println(regex_match("\\w+", "word_123"))
        println(regex_find("[0-9]+", "abc123def456"))
        println(regex_find("[0-9]+", "no digits"))

        println(regex_is_valid("[a-z]+"))
        println(regex_is_valid("["))
        println(regex_match("[", "abc"))
        println(regex_find("[", "abc"))
        println(len(regex_find_all("[", "abc")))
        println(regex_replace("[", "abc", "X"))
        println(regex_replace_all("[", "abc", "X"))
        let invalid_split = regex_split("[", "abc")
        println(len(invalid_split))
        println(invalid_split[0])
        println(len(regex_captures("[", "abc")))
        "#,
        &[
            "true", "false", "true", "123", "", "true", "false", "false", "", "0", "abc", "abc",
            "1", "abc", "0",
        ],
    );
}

#[test]
fn regex_find_all_and_captures_preserve_order_and_groups() {
    assert_lines(
        r#"
        let matches = regex_find_all("[0-9]+", "z12a3b45")
        println(len(matches))
        println(matches[0])
        println(matches[1])
        println(matches[2])

        let captures = regex_captures("([a-z]+)([0-9]+)", "abc42")
        println(len(captures))
        println(captures[0])
        println(captures[1])
        println(captures[2])

        // Unmatched optional groups are omitted, matching the VM's
        // filter_map over capture slots rather than producing empty strings.
        let optional = regex_captures("(a)?(b)", "b")
        println(len(optional))
        println(optional[0])
        println(optional[1])
        "#,
        &[
            "3", "12", "3", "45", "3", "abc42", "abc", "42", "2", "b", "b",
        ],
    );
}

#[test]
fn regex_replacement_split_shorthand_and_anchors_match_vm_semantics() {
    assert_lines(
        r#"
        println(regex_replace("([a-z]+)([0-9]+)", "abc12 xyz34", "$2-$1"))
        println(regex_replace_all("([a-z]+)([0-9]+)", "abc12 xyz34", "$2-$1"))
        println(regex_replace_all("[0-9]+", "a1b22", "[$0]/$$"))

        let parts = regex_split("[,;]", "a,b;c,")
        println(len(parts))
        println(parts[0])
        println(parts[1])
        println(parts[2])
        println(parts[3])

        println(regex_find("\\s+", "a  b"))
        println(regex_find("\\w+", "--word_123--"))
        println(regex_match("^first\\s+last$", "first  last"))
        println(regex_match("^first\\s+last$", "xfirst  last"))
        "#,
        &[
            "12-abc xyz34",
            "12-abc 34-xyz",
            "a[1]/$b[22]/$",
            "4",
            "a",
            "b",
            "c",
            "",
            "  ",
            "word_123",
            "true",
            "false",
        ],
    );
}

#[test]
fn hash_and_encoding_builtins_match_the_reference_digests() {
    // Known-answer tests: these are the published digests for "hello" and the
    // RFC 4648 vectors, so they pin the C implementations rather than merely
    // comparing them against themselves.
    assert_lines(
        r#"
        println(md5("hello"))
        println(sha1("hello"))
        println(sha256("hello"))
        println(base64_encode("Lira"))
        println(base64_decode("TGlyYQ=="))
        println(base64_encode(""))
        println(url_encode("a b&c"))
        println(url_decode("a+b%26c"))
        "#,
        &[
            "5d41402abc4b2a76b9719d911017c592",
            "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d",
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
            "TGlyYQ==",
            "Lira",
            "",
            "a+b%26c",
            "a b&c",
        ],
    );
}

#[test]
fn sha512_matches_the_reference_digest() {
    assert_lines(
        r#"println(sha512("hello"))"#,
        &["9b71d224bd62f3785d96d46ad3ea3d73319bfbc2890caadae2dff72519673ca72323c3d99ba5c11d7c7acc6e14b8c5da0c4663475c2e5c3adef46f73bcdec043"],
    );
}

#[test]
fn uuids_are_well_formed_and_versioned() {
    assert_lines(
        r#"
        println(uuid_is_valid(uuid_v4()))
        println(uuid_is_valid(uuid_v7()))
        println(uuid_is_valid("not-a-uuid"))
        println(uuid_nil())
        println(len(uuid_v4()))
        "#,
        &[
            "true",
            "true",
            "false",
            "00000000-0000-0000-0000-000000000000",
            "36",
        ],
    );
}

#[test]
fn file_and_filesystem_builtins_round_trip() {
    assert_lines(
        r#"
        let dir = env_temp_dir() + "/lira-native-file-test"
        mkdir_all(dir)
        let path = dir + "/note.txt"
        let handle = file_open(path, 1)
        file_write(handle, "written natively")
        file_close(handle)
        println(file_exists(path))
        println(is_file(path))
        println(is_dir(dir))
        let reader = file_open(path, 0)
        println(file_read(reader, 100))
        file_close(reader)
        println(file_size(path))
        remove_all(dir)
        println(file_exists(path))
        "#,
        &["true", "true", "true", "written natively", "16", "false"],
    );
}

#[test]
fn concurrent_file_handles_round_trip_distinct_contents_in_aot_and_jit() {
    let dir = scratch_dir();
    let base = dir.to_str().expect("temporary path is UTF-8");
    let mut spawns = String::new();
    for index in 0..8 {
        spawns.push_str(&format!(
            "            spawn file_worker(\"{base}/file-{index}.txt\", \"body-{index}\", ch)\n"
        ));
    }
    let source = format!(
        r#"
        fn file_worker(path: string, body: string, ch: Channel<int>) {{
            let handle = file_open(path, 1)
            if handle < 0 {{
                send(ch, 0)
            }} else {{
                let wrote = file_write(handle, body)
                let closed = file_close(handle)
                let reader = file_open(path, 0)
                let got = file_read(reader, 128)
                let reopened = file_close(reader)
                if wrote == len(body) && closed && reopened && got == body {{
                    send(ch, 1)
                }} else {{
                    send(ch, 0)
                }}
            }}
        }}
        fn main() {{
            let dir = "{base}"
            mkdir_all(dir)
            let ch: Channel<int> = chan(8)
{spawns}
            var total = 0
            for i in 0..8 {{ total = total + recv(ch) }}
            println(total)
            remove_all(dir)
        }}
        "#
    );
    let output = run_native(&source).expect("concurrent file AOT source runs");
    assert_eq!(output.lines().collect::<Vec<_>>(), ["8"]);

    let jit_source = source.replace(
        "            println(total)\n",
        "            if total != 8 { println(1 / 0) }\n",
    );
    assert_jit_success(&jit_source);
}

#[test]
fn filesystem_metadata_and_mutation_are_async_and_report_errors() {
    let source = r#"
        let dir = env_temp_dir() + "/lira-native-fs-async"
        mkdir_all(dir)
        let source_path = dir + "/source.txt"
        let renamed = dir + "/renamed.txt"
        let copied = dir + "/copied.txt"
        let handle = file_open(source_path, 1)
        file_write(handle, "filesystem")
        file_close(handle)
        println(file_exists(source_path))
        println(file_size(source_path) == 10)
        println(is_file(source_path))
        println(is_dir(dir))
        println(rename(source_path, renamed))
        println(copy(renamed, copied))
        println(len(listdir(dir)) == 2)
        println(remove(renamed))
        println(remove(copied))
        println(file_exists(renamed) == false)
        println(file_size("/path/that/does/not/exist") == -1)
        remove_all(dir)
    "#;
    assert_lines(
        source,
        &[
            "true", "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
        ],
    );
    let jit_source = r#"
        fn main() {
            let dir = env_temp_dir() + "/lira-native-fs-jit-async"
            mkdir_all(dir)
            let path = dir + "/file.txt"
            let h = file_open(path, 1)
            let wrote = file_write(h, "ok")
            file_close(h)
            let renamed = dir + "/renamed.txt"
            let copied = dir + "/copied.txt"
            if wrote != 2 || !rename(path, renamed) || !copy(renamed, copied) || len(listdir(dir)) != 2 || file_size("/missing") != -1 { println(1 / 0) }
            remove_all(dir)
        }
    "#;
    assert_jit_success(jit_source);
}

#[test]
fn invalid_and_busy_native_handles_are_rejected() {
    assert_lines(
        r#"
        println(file_read(999, 10) == "")
        println(file_write(999, "data") == -1)
        println(file_close(999) == false)
        println(tcp_read(999, 10) == "")
        println(tcp_write(999, "data") == -1)
        println(tcp_close(999) == false)
        "#,
        &["true", "true", "true", "true", "true", "true"],
    );
}

#[test]
fn worker_result_alloc_failure_repairs_busy_file_and_tcp_handles() {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("create allocation-failure fixture directory");
    let file_path = dir.join("busy.txt");
    std::fs::write(&file_path, b"payload").expect("write allocation-failure fixture");
    let source = format!(
        r#"
        let h = file_open("{}", 0)
        env_set("LIRA_TEST_FAIL_FILE_READ_RESULT", "1")
        println(file_read(h, 32) == "")
        env_remove("LIRA_TEST_FAIL_FILE_READ_RESULT")
        println(file_close(h))
        "#,
        file_path.display()
    );
    assert_lines(&source, &["true", "true"]);
    let _ = std::fs::remove_dir_all(&dir);

    let jit_dir = scratch_dir();
    std::fs::create_dir_all(&jit_dir).expect("create JIT allocation-failure directory");
    let jit_path = jit_dir.join("busy.txt");
    std::fs::write(&jit_path, b"payload").expect("write JIT allocation-failure fixture");
    let jit_source = format!(
        r#"
        fn main() {{
            let h = file_open("{}", 0)
            env_set("LIRA_TEST_FAIL_FILE_READ_RESULT", "1")
            if file_read(h, 32) != "" {{ println(1 / 0) }}
            env_remove("LIRA_TEST_FAIL_FILE_READ_RESULT")
            if !file_close(h) {{ println(1 / 0) }}
        }}
        "#,
        jit_path.display()
    );
    assert_jit_success(&jit_source);
    let _ = std::fs::remove_dir_all(&jit_dir);

    let (port, server) = start_invalid_byte_tcp_server(b"payload");
    let tcp_source = format!(
        r#"
        let h = tcp_connect("127.0.0.1", {port})
        env_set("LIRA_TEST_FAIL_TCP_READ_RESULT", "1")
        println(tcp_read(h, 32) == "")
        env_remove("LIRA_TEST_FAIL_TCP_READ_RESULT")
        println(tcp_close(h))
        "#
    );
    assert_lines(&tcp_source, &["true", "true"]);
    server
        .join()
        .expect("AOT allocation-failure TCP server completed");

    let (port, server) = start_invalid_byte_tcp_server(b"payload");
    let tcp_jit_source = format!(
        r#"
        fn main() {{
            let h = tcp_connect("127.0.0.1", {port})
            env_set("LIRA_TEST_FAIL_TCP_READ_RESULT", "1")
            if tcp_read(h, 32) != "" {{ println(1 / 0) }}
            env_remove("LIRA_TEST_FAIL_TCP_READ_RESULT")
            if !tcp_close(h) {{ println(1 / 0) }}
        }}
        "#
    );
    assert_jit_success(&tcp_jit_source);
    server
        .join()
        .expect("JIT allocation-failure TCP server completed");
}

#[test]
fn file_read_is_strict_and_tcp_read_is_lossy_for_invalid_utf8() {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let file_path = dir.join("invalid.bin");
    std::fs::write(&file_path, [0xff_u8, b'X']).expect("write invalid UTF-8 fixture");
    let source = format!(
        r#"
        let h = file_open("{}", 0)
        println(file_read(h, 16) == "")
        file_close(h)
        "#,
        file_path.display()
    );
    assert_lines(&source, &["true"]);
    let _ = std::fs::remove_dir_all(&dir);

    let (port, server) = start_invalid_byte_tcp_server(&[0xff, b'X']);
    let tcp_source = format!(
        r#"
        let h = tcp_connect("127.0.0.1", {port})
        let body = tcp_read(h, 16)
        println(body == "�X")
        tcp_close(h)
        "#
    );
    assert_lines(&tcp_source, &["true"]);
    server.join().expect("invalid-byte TCP server completed");

    let (port, server) = start_invalid_byte_tcp_server(&[0xff, b'X']);
    let jit_source = format!(
        r#"
        fn main() {{
            let h = tcp_connect("127.0.0.1", {port})
            let body = tcp_read(h, 16)
            if body != "�X" {{ println(1 / 0) }}
            tcp_close(h)
        }}
        "#
    );
    assert_jit_success(&jit_source);
    server
        .join()
        .expect("invalid-byte JIT TCP server completed");
}

#[test]
fn tcp_lossy_utf8_matches_vm_for_all_invalid_scalar_boundaries() {
    let cases: &[(&[u8], &str, bool)] = &[
        (&[0xe0, 0x80, 0x80], "���", true),
        (&[0xed, 0xa0, 0x80], "���", true),
        (&[0xf4, 0x90, 0x80, 0x80], "����", true),
        (&[0xe2, 0x82], "�", true),
        (
            &[
                0xe0, 0xa0, 0x80, 0xed, 0x9f, 0xbf, 0xf0, 0x90, 0x80, 0x80, 0xf4, 0x8f, 0xbf, 0xbf,
            ],
            "ࠀ퟿𐀀􏿿",
            false,
        ),
    ];
    for &(bytes, expected, invalid) in cases {
        let dir = scratch_dir();
        std::fs::create_dir_all(&dir).expect("create UTF-8 file fixture directory");
        let file_path = dir.join("invalid.bin");
        std::fs::write(&file_path, bytes).expect("write UTF-8 file fixture");
        let file_source = format!(
            r#"
            let h = file_open("{}", 0)
            println(file_read(h, 32) == "{}")
            file_close(h)
            "#,
            file_path.display(),
            if invalid { "" } else { expected }
        );
        assert_lines(&file_source, &["true"]);
        let vm_file = run_bytecode(&file_source).expect("VM strict file source ran");
        assert_eq!(
            vm_file.trim(),
            "true",
            "VM strict file result for {bytes:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);

        let (port, server) = start_invalid_byte_tcp_server(bytes);
        let source = format!(
            r#"
            let h = tcp_connect("127.0.0.1", {port})
            println(tcp_read(h, 32) == "{expected}")
            tcp_close(h)
            "#
        );
        assert_lines(&source, &["true"]);
        server.join().expect("AOT UTF-8 server completed");

        let (port, server) = start_invalid_byte_tcp_server(bytes);
        let vm_source = format!(
            r#"
            let h = tcp_connect("127.0.0.1", {port})
            println(tcp_read(h, 32) == "{expected}")
            tcp_close(h)
            "#
        );
        let vm = run_bytecode(&vm_source).expect("VM UTF-8 source ran");
        assert_eq!(vm.trim(), "true", "VM UTF-8 result for {bytes:?}");
        server.join().expect("VM UTF-8 server completed");

        let (port, server) = start_invalid_byte_tcp_server(bytes);
        let jit_source = format!(
            r#"
            fn main() {{
                let h = tcp_connect("127.0.0.1", {port})
                if tcp_read(h, 32) != "{expected}" {{ println(1 / 0) }}
                tcp_close(h)
            }}
            "#
        );
        assert_jit_success(&jit_source);
        server.join().expect("JIT UTF-8 server completed");
    }
}

#[test]
fn busy_tcp_handle_rejects_overlapping_write_in_aot_and_jit() {
    let (port, server) = start_delayed_tcp_echo_server(1);
    let first_port = port;
    let source = format!(
        r#"
        fn reader(handle: int, ch: Channel<string>) {{
            let body = tcp_read(handle, 32)
            send(ch, body)
        }}
        fn main() {{
            let handle = tcp_connect("127.0.0.1", {port})
            tcp_write(handle, "tcp-ping!")
            let ch: Channel<string> = chan(1)
            spawn reader(handle, ch)
            fiber_yield()
            println(tcp_write(handle, "x") == -1)
            let body = recv(ch)
            println(body == "echo-ping")
            tcp_close(handle)
        }}
        "#
    );
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("busy TCP AOT scratch directory");
    let source_path = dir.join("program.li");
    std::fs::write(&source_path, &source).expect("busy TCP AOT source");
    let output = common::run_aot(&source_path, &source).expect("run busy TCP AOT binary");
    let _ = std::fs::remove_dir_all(&dir);
    server.join().expect("busy TCP server completed");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .collect::<Vec<_>>(),
        ["true", "true"]
    );

    let (port, server) = start_delayed_tcp_echo_server(1);
    let jit_source = source.replace(&first_port.to_string(), &port.to_string());
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join("busy-tcp.li");
    std::fs::write(&path, &jit_source).expect("write busy TCP source");
    let result = common::run_jit(path.to_str().expect("utf-8 path"), &jit_source);
    let _ = std::fs::remove_dir_all(&dir);
    server.join().expect("busy TCP JIT server completed");
    assert_eq!(result, Ok(0), "busy TCP JIT source failed: {result:?}");
}

#[test]
fn environment_and_time_builtins_report_live_values() {
    assert_lines(
        r#"
        env_set("LIRA_NATIVE_TEST", "present")
        println(env_has("LIRA_NATIVE_TEST"))
        println(env_get("LIRA_NATIVE_TEST"))
        env_remove("LIRA_NATIVE_TEST")
        println(env_has("LIRA_NATIVE_TEST"))
        println(time_ms() > 0)
        println(time_secs() > 0)
        println(random() < 1.0)
        let n = random_int(5, 10)
        println(n >= 5 && n < 10)
        "#,
        &["true", "present", "false", "true", "true", "true", "true"],
    );
}

#[test]
fn modulo_mixed_operands_have_vm_native_parity() {
    // Mixed int/float modulo must evaluate identically on the bytecode VM and
    // the native backend, and must never be rejected. Used to diverge: the VM
    // errored with "Cannot modulo int by float" while native computed it.
    assert_any_parity(
        "println(10 % 2.5)\nprintln(10.5 % 2)\nprintln(10 % 3)\nprintln(10.5 % 3.0)",
        &["0", "0.5", "1", "1.5"],
    );
}

#[test]
fn time_from_components_has_vm_native_parity_and_fails_closed() {
    // A valid UTC date yields the same epoch millis on both backends, and an
    // unrepresentable year fails closed to 0 on both (the native C path used to
    // have signed-overflow UB for extreme years; the VM used to *silently
    // truncate* the year to a plausible-but-wrong date — both now agree).
    assert_any_parity(
        "println(time_from_components(2020, 1, 2, 3, 4, 5))\nprintln(time_from_components(-9223372036854775808, 1, 1, 0, 0, 0))",
        &["1577934245000", "0"],
    );
}

#[test]
fn random_int_reversed_bounds_is_deterministic_parity() {
    // `min > max` returns `min` on both backends (deterministic, so parity is
    // exact). Exercises the same C path that used to have unsigned-range math
    // and keeps native and VM in lockstep.
    assert_any_parity(
        "println(random_int(7, 3))\nprintln(random_int(42, 42))",
        &["7", "42"],
    );
}

#[test]
fn random_int_full_domain_and_large_ranges_stay_bounded_in_jit() {
    // The native overflow fix must keep `random_int` bounded for ranges that
    // previously tripped signed-overflow UB (span > 2^63) and the precision-
    // loss overshoot near i64::MAX. The program validates in-language and
    // reports via exit code.
    assert_jit_success(
        r#"
        fn main() {
            var i = 0
            var min: int = -9223372036854775808
            var max: int = 9223372036854775807
            while i < 200 {
                let r = random_int(min, max)
                if r < min || r > max { println(1 / 0) }
                i = i + 1
            }
            let near_max = random_int(9223372036854775807 - 10000, 9223372036854775807)
            if near_max < 9223372036854775807 - 10000 || near_max > 9223372036854775807 { println(1 / 0) }
            println("ok")
        }
        "#,
    );
}

#[test]
fn time_from_components_extreme_year_fails_closed_in_jit() {
    // The native C path used to have signed-overflow UB for a year near
    // i64::MIN (the `year - 1900` subtraction overflowed an int64_t). It must
    // now fail closed to 0. (An invalid-but-normalizable month/day is a
    // separate pre-existing `timegm` normalization behavior, not this overflow;
    // it is documented as a known divergence rather than asserted here.)
    assert_jit_success(
        r#"
        fn main() {
            let year_min = time_from_components(-9223372036854775808, 1, 1, 0, 0, 0)
            if year_min != 0 { println(1 / 0) }
            println("ok")
        }
        "#,
    );
}

#[test]
fn a_user_function_shadows_a_built_in_of_the_same_name() {
    assert_lines(
        r#"
        fn random() -> int { return 4 }
        println(random())
        "#,
        &["4"],
    );
}

// ---------------------------------------------------------------------- //
// Type resolution                                                         //
// ---------------------------------------------------------------------- //

#[test]
fn type_aliases_are_transparent() {
    assert_lines(
        r#"
        type Integer = int
        type Text = string
        fn twice(n: Integer) -> Integer { return n * 2 }
        let label: Text = "answer"
        println(label + ": " + twice(21))
        "#,
        &["answer: 42"],
    );
}

#[test]
fn ranges_are_values_as_well_as_loop_subjects() {
    assert_lines(
        r#"
        let r = 1..4
        println(r.start)
        println(r.end)
        println(r.inclusive)
        println("inclusive=" + r.inclusive)
        var total = 0
        for i in r { total = total + i }
        println(total)
        let inclusive = 1..=4
        var sum = 0
        for i in inclusive { sum = sum + i }
        println(sum)
        "#,
        &["1", "4", "false", "inclusive=false", "6", "10"],
    );
}

#[test]
fn impl_blocks_on_built_in_types_dispatch() {
    assert_lines(
        r#"
        impl int {
            fn doubled(self) -> int { return self * 2 }
        }
        impl string {
            fn shout(self) -> string { return self + "!" }
        }
        println(21.doubled())
        println("hey".shout())
        "#,
        &["42", "hey!"],
    );
}

// ---------------------------------------------------------------------- //
// Tuples                                                                  //
// ---------------------------------------------------------------------- //

#[test]
fn tuples_carry_a_type_per_position() {
    assert_lines(
        r#"
        let pair = (1, "two")
        let (n, s) = pair
        println(n)
        println(s)
        fn swap(p: (int, int)) -> (int, int) {
            let (a, b) = p
            return (b, a)
        }
        let (x, y) = swap((3, 4))
        println(x)
        println(y)
        "#,
        &["1", "two", "4", "3"],
    );
}

#[test]
fn tuple_patterns_nest_and_test_literals() {
    assert_lines(
        r#"
        fn quadrant(p: (int, int)) -> string {
            return match p {
                (0, 0) => "origin",
                (0, y) => "yaxis",
                (x, 0) => "xaxis",
                (x, y) => "other"
            }
        }
        println(quadrant((0, 0)))
        println(quadrant((0, 5)))
        println(quadrant((5, 0)))
        println(quadrant((5, 5)))
        let nested = ((1, 2), 3)
        match nested {
            ((a, b), c) => println(a + b + c)
        }
        "#,
        &["origin", "yaxis", "xaxis", "other", "6"],
    );
}

#[test]
fn struct_patterns_destructure_in_a_let() {
    assert_lines(
        r#"
        struct Point { x: int, y: int }
        let p = Point { x: 7, y: 8 }
        let { x, y } = p
        println(x + y)
        "#,
        &["15"],
    );
}

// ---------------------------------------------------------------------- //
// Lambdas and closures                                                    //
// ---------------------------------------------------------------------- //

#[test]
fn lambdas_are_callable_values() {
    assert_lines(
        r#"
        let double = |x: int| x * 2
        println(double(5))
        let add = |a: int, b: int| a + b
        println(add(3, 4))
        let get_ten = || 10
        println(get_ten())
        println((|x: int| x * x)(4))
        "#,
        &["10", "7", "10", "16"],
    );
}

#[test]
fn closures_capture_by_value_and_outlive_their_frame() {
    // `make_adder`'s frame is gone by the time `add5` runs, so `n` has to have
    // been copied into the closure rather than referenced on the stack.
    assert_lines(
        r#"
        fn make_adder(n: int) -> fn(int) -> int {
            return |x: int| x + n
        }
        let add5 = make_adder(5)
        let add10 = make_adder(10)
        println(add5(3))
        println(add10(3))
        println(add5(7))

        fn make_linear(a: int, b: int) -> fn(int) -> int {
            return |x: int| a * x + b
        }
        let f = make_linear(2, 3)
        println(f(0))
        println(f(10))
        "#,
        &["8", "13", "12", "3", "23"],
    );
}

#[test]
fn a_named_function_can_be_passed_as_a_value() {
    assert_lines(
        r#"
        fn double(x: int) -> int { return x * 2 }
        fn square(x: int) -> int { return x * x }
        fn apply_twice(f: fn(int) -> int, x: int) -> int { return f(f(x)) }
        fn compose(f: fn(int) -> int, g: fn(int) -> int, x: int) -> int { return f(g(x)) }
        println(apply_twice(double, 3))
        println(apply_twice(square, 2))
        println(compose(double, square, 3))
        // A lambda and a named function are the same kind of value.
        println(apply_twice(|x: int| x + 10, 0))
        "#,
        &["12", "16", "18", "20"],
    );
}

#[test]
fn a_captured_name_shadowed_inside_the_body_is_not_captured() {
    assert_lines(
        r#"
        let n = 1
        let f = || {
            let n = 99
            return n
        }
        println(f())
        println(n)
        "#,
        &["99", "1"],
    );
}

// ---------------------------------------------------------------------- //
// Optionals and Result                                                    //
// ---------------------------------------------------------------------- //

#[test]
fn scalar_optionals_are_boxed_so_null_has_a_representation() {
    assert_lines(
        r#"
        fn get_value() -> int? { return 42 }
        fn get_null() -> int? { return null }
        fn get_string() -> string? { return null }
        println(get_value() ?? 0)
        println(get_null() ?? 0)
        println(get_string() ?? "default")
        println(get_value())
        println(get_null())
        println("value: " + get_value())
        println("value: " + get_null())
        "#,
        &[
            "42",
            "0",
            "default",
            "42",
            "null",
            "value: 42",
            "value: null",
        ],
    );
}

#[test]
fn the_try_operator_propagates_an_absent_optional() {
    assert_lines(
        r#"
        fn get_some() -> int? { return 42 }
        fn get_none() -> int? { return null }
        fn try_some() -> int? {
            let x = get_some()?
            return x + 1
        }
        fn try_none() -> int? {
            let x = get_none()?
            return x + 1
        }
        println(try_some())
        println(try_none())
        "#,
        &["43", "null"],
    );
}

#[test]
fn result_carries_the_payload_types_from_its_context() {
    assert_lines(
        r#"
        fn divide(a: int, b: int) -> Result<int, string> {
            if b == 0 {
                return Result::Err("division by zero")
            }
            return Result::Ok(a / b)
        }
        fn calculate(x: int, y: int) -> Result<int, string> {
            let result = divide(x, y)?
            return Result::Ok(result * 10)
        }
        match calculate(100, 10) {
            Result::Ok(v) => println(v),
            Result::Err(e) => println("error: " + e)
        }
        match calculate(1, 0) {
            Result::Ok(v) => println(v),
            Result::Err(e) => println("error: " + e)
        }
        "#,
        &["100", "error: division by zero"],
    );
}

#[test]
fn optional_chaining_short_circuits_on_null() {
    assert_lines(
        r#"
        struct Person { name: string, age: int }
        let p = Person { name: "Alice", age: 30 }
        println("valid: " + p?.name)
        println("null: " + null?.name)
        "#,
        &["valid: Alice", "null: null"],
    );
}

#[test]
fn null_coalescing_leaves_a_non_nullable_value_alone() {
    assert_lines(
        r#"
        println(null ?? 42)
        println(100 ?? 42)
        "#,
        &["42", "100"],
    );
}

#[test]
fn a_function_body_may_end_in_a_bare_expression() {
    assert_lines(
        r#"
        fn describe(x: int) -> string {
            match x {
                0 => "zero",
                1 => "one",
                _ => "other"
            }
        }
        println(describe(0))
        println(describe(1))
        println(describe(42))
        "#,
        &["zero", "one", "other"],
    );
}

// ---------------------------------------------------------------------- //
// Maps                                                                    //
// ---------------------------------------------------------------------- //

#[test]
fn maps_are_keyed_by_string() {
    assert_lines(
        r#"
        let m = { "name": "Alice", "city": "Oslo" }
        println(m["name"])
        println(m["city"])
        println(len(m))
        m["city"] = "Bergen"
        println(m["city"])
        println(len(m))
        // A key that was never set reads as the zero value, which for a
        // reference is null.
        println(m["missing"])
        "#,
        &["Alice", "Oslo", "2", "Bergen", "2", "null"],
    );
}

#[test]
fn maps_grow_past_their_initial_capacity() {
    assert_lines(
        r#"
        let m = { "k0": 0 }
        var i = 1
        while i < 50 {
            m["k" + i] = i * i
            i = i + 1
        }
        println(len(m))
        println(m["k7"])
        println(m["k49"])
        "#,
        &["50", "49", "2401"],
    );
}

// ---------------------------------------------------------------------- //
// select                                                                  //
// ---------------------------------------------------------------------- //

#[test]
fn seeded_ready_receive_arbitration_matches_vm_and_preserves_source_ordinals() {
    let source = r#"
        fn main() {
            let a: Channel<int> = chan(1)
            let b: Channel<int> = chan(1)
            send(a, 11)
            send(b, 22)
            select {
                x = <-a => println("a=" + x)
                _ => println("default")
                y = <-b => println("b=" + y)
            }
        }
    "#;

    // The default occupies ordinal 1 even though it has no descriptor. Seeds
    // 1 and 2 therefore pin opposite winners only when the native scorer uses
    // the original source ordinals, as the VM does.
    assert_seeded_select_aot_vm(source, &[(1, &["a=11"]), (2, &["b=22"])]);
}

#[test]
fn seeded_duplicate_send_arms_commit_the_matching_value_and_body() {
    let source = r#"
        fn main() {
            let ch: Channel<int> = chan(1)
            select {
                11 -> ch => println("first")
                22 -> ch => println("second")
            }
            println(recv(ch))
        }
    "#;

    assert_seeded_select_aot_vm(source, &[(1, &["first", "11"]), (2, &["second", "22"])]);
}

#[test]
fn seeded_mixed_ready_arms_commit_exactly_one_operation() {
    let source = r#"
        fn main() {
            let recv_ch: Channel<int> = chan(1)
            let send_ch: Channel<int> = chan(1)
            send(recv_ch, 7)
            select {
                value = <-recv_ch => println("recv=" + value)
                9 -> send_ch => println("send")
            }
            select {
                value = <-send_ch => println("sent=" + value)
                _ => println("no-send")
            }
            select {
                value = <-recv_ch => println("remaining=" + value)
                _ => println("no-recv")
            }
        }
    "#;

    assert_seeded_select_aot_vm(
        source,
        &[
            (1, &["recv=7", "no-send", "no-recv"]),
            (3, &["send", "sent=9", "remaining=7"]),
        ],
    );
}

#[test]
fn select_takes_the_default_arm_when_nothing_is_ready() {
    assert_lines(
        r#"
        let ch = chan(1)
        select {
            _ => println("nothing ready")
        }
        "#,
        &["nothing ready"],
    );
}

#[test]
fn select_prefers_a_ready_channel_over_the_default() {
    assert_lines(
        r#"
        fn main() {
            let ch = chan(1)
            send(ch, 7)
            select {
                v = <-ch => println("got " + v)
                _ => println("nothing ready")
            }
        }
        "#,
        &["got 7"],
    );
}

#[test]
fn a_select_without_a_default_waits_for_a_sender() {
    assert_lines(
        r#"
        fn producer(ch) {
            send(ch, 99)
        }
        fn main() {
            let ch = chan(0)
            spawn producer(ch)
            select {
                v = <-ch => println("received " + v)
            }
        }
        "#,
        &["received 99"],
    );
}

#[test]
fn select_waits_for_a_sender_while_io_is_pending() {
    assert_lines(
        r#"
        fn producer(ch: Channel<int>) {
            sleep(160)
            send(ch, 99)
        }
        fn main() {
            let ch: Channel<int> = chan(0)
            spawn producer(ch)
            select {
                v = <-ch => println("received " + v)
            }
        }
        "#,
        &["received 99"],
    );
}

#[test]
fn select_receives_from_a_closed_empty_channel_in_aot_jit_and_vm() {
    let native_value = r#"
        fn main() {
            let ch: Channel<int> = chan(0)
            close(ch)
            select {
                v = <-ch => println(v == 0)
                _ => println("default")
            }
        }
    "#;
    assert_lines(native_value, &["true"]);

    let source = r#"
        fn main() {
            let ch: Channel<int> = chan(0)
            close(ch)
            select {
                v = <-ch => println("closed")
                _ => println("default")
            }
        }
    "#;
    assert_any_parity(source, &["closed"]);

    let jit_source = r#"
        fn main() {
            let ch: Channel<int> = chan(0)
            close(ch)
            select {
                v = <-ch => println("closed")
                _ => println(1 / 0)
            }
        }
    "#;
    assert_jit_success(jit_source);

    let jit_value = r#"
        fn main() {
            let ch: Channel<int> = chan(0)
            close(ch)
            select {
                v = <-ch => { if v != 0 { println(1 / 0) } }
                _ => println(1 / 0)
            }
        }
    "#;
    assert_jit_success(jit_value);
}

#[test]
fn blocked_send_failure_on_close_stops_every_backend_before_later_output() {
    let source = r#"
        fn sender(ch: Channel<int>) {
            send(ch, 7)
            println("after send")
        }
        fn main() {
            let ch: Channel<int> = chan(0)
            spawn sender(ch)
            fiber_yield()
            close(ch)
            println("closed")
        }
    "#;

    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("blocked-send AOT scratch directory");
    let source_path = dir.join("program.li");
    std::fs::write(&source_path, source).expect("blocked-send AOT source");
    let output = common::run_aot(&source_path, source).expect("run blocked-send AOT binary");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        output.stdout.is_empty(),
        "output after failed close must not run"
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("after send"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("send on closed channel"));

    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join("blocked-send.li");
    std::fs::write(&path, source).expect("write blocked-send source");
    let jit_result = common::run_jit(path.to_str().expect("utf-8 path"), source)
        .expect("JIT blocked-send source should execute");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(jit_result, 1);

    let bytecode = lirac::compile(source).expect("VM close-race source compiles");
    let (vm_output, vm_error) = liravm::run_with_capture_structured(&bytecode)
        .expect_err("a blocked child send must propagate when its channel closes");
    assert!(
        vm_output.is_empty(),
        "VM output after failed close must not run"
    );
    assert_eq!(vm_error.message, "send on closed channel");
    assert_eq!(vm_error.line, Some(3));
    assert_eq!(vm_error.column, Some(13));
    assert_eq!(vm_error.stack, ["sender"]);
}

#[test]
fn a_select_that_can_never_be_ready_reports_a_deadlock() {
    let output = run_native(
        r#"
        fn main() {
            let ch = chan(0)
            println("waiting")
            select {
                v = <-ch => println("never")
            }
        }
        "#,
    )
    .expect("compiles");
    assert!(output.contains("waiting"), "unexpected output: {}", output);
    assert!(output.contains("deadlock"), "unexpected output: {}", output);
    assert!(!output.contains("never"), "unexpected output: {}", output);
}

#[test]
fn select_receive_binder_preserves_string_payload_type() {
    assert_lines(
        r#"
        fn main() {
            let ch: Channel<string> = chan(1)
            send(ch, "selected")
            select {
                value = <-ch => println(value + "!")
                _ => println("wrong arm")
            }
        }
        "#,
        &["selected!"],
    );
}

#[test]
fn select_receive_binder_preserves_struct_payload_fields() {
    assert_lines(
        r#"
        struct Page {
            status: int
            body: string
        }
        fn main() {
            let pages: Channel<Page> = chan(1)
            send(pages, Page { status: 201, body: "created" })
            select {
                page = <-pages => println("" + page.status + " " + page.body)
                _ => println("wrong arm")
            }
        }
        "#,
        &["201 created"],
    );
}

#[test]
fn select_send_and_receive_int_round_trips_through_aot() {
    assert_lines(
        r#"
        fn main() {
            let ch = chan(1)
            select { 41 -> ch => {} }
            select {
                value = <-ch => println(value + 1)
                _ => println("wrong arm")
            }
        }
        "#,
        &["42"],
    );
}

#[test]
fn select_send_and_receive_int_round_trips_through_jit() {
    assert_jit_success(
        r#"
        fn main() {
            let ch = chan(1)
            select { 41 -> ch => {} }
            select {
                value = <-ch => { if value != 41 { println("wrong value") } }
                _ => println("wrong arm")
            }
        }
        "#,
    );
}

#[test]
fn select_send_and_receive_string_round_trips_through_aot() {
    assert_lines(
        r#"
        fn main() {
            let ch = chan(1)
            select { "selected" -> ch => {} }
            select {
                value = <-ch => println(value + "!")
                _ => println("wrong arm")
            }
        }
        "#,
        &["selected!"],
    );
}

#[test]
fn select_send_and_receive_string_round_trips_through_jit() {
    assert_jit_success(
        r#"
        fn main() {
            let ch = chan(1)
            select { "selected" -> ch => {} }
            select {
                value = <-ch => { if value != "selected" { println("wrong value") } }
                _ => println("wrong arm")
            }
        }
        "#,
    );
}

#[test]
fn tail_select_method_uses_typed_channel_field_storage() {
    assert_lines(
        r#"
        struct BoxedInt {
            ch: Channel<int>
        }
        impl BoxedInt {
            fn put(self, value: int) {
                select { value -> self.ch => {} }
            }
            fn take(self) -> int {
                select { value = <-self.ch => { return value } }
                return 0
            }
        }
        fn main() {
            let b = BoxedInt { ch: chan(1) }
            b.put(73)
            println(b.take())
        }
        "#,
        &["73"],
    );
}

#[test]
fn select_send_mixed_channel_type_is_rejected() {
    assert_rejected(
        r#"
        fn main() {
            let ch: Channel<int> = chan(1)
            select { "not an int" -> ch => {} }
        }
        "#,
        "Argument type mismatch",
    );
}

fn example_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name)
}

#[test]
fn sync_mutex_waitgroup_example_runs_natively() {
    let output = run_native_file(&example_path("sync_mutex_waitgroup.li")).expect("native example");
    assert_eq!(
        output.lines().collect::<Vec<_>>(),
        ["total: 2000", "expected: 2000"]
    );
}

#[test]
fn sync_semaphore_example_runs_natively() {
    let output = run_native_file(&example_path("sync_semaphore.li")).expect("native example");
    assert_eq!(
        output.lines().collect::<Vec<_>>(),
        ["limit: 2", "max concurrent: 2", "BOUNDED: ok"]
    );
}

#[test]
fn sync_try_lock_example_runs_natively() {
    let output = run_native_file(&example_path("sync_try_lock.li")).expect("native example");
    assert_eq!(
        output.lines().collect::<Vec<_>>(),
        ["acquired 5", "busy", "acquired 6"]
    );
}

#[test]
fn sync_with_closure_example_runs_natively() {
    let output = run_native_file(&example_path("sync_with_closure.li")).expect("native example");
    assert_eq!(output.lines().collect::<Vec<_>>(), ["with result: 42"]);
}

#[test]
fn receiving_from_a_closed_typed_channel_yields_the_null_boundary_value() {
    assert_lines(
        r#"
        fn main() {
            let ch: Channel<string> = chan(1)
            close(ch)
            println(recv(ch))
        }
        "#,
        &["null"],
    );
}

#[test]
fn incompatible_channel_send_is_rejected_before_native_codegen() {
    assert_rejected(
        r#"
        fn main() {
            let ch: Channel<string> = chan(1)
            send(ch, 42)
        }
        "#,
        "Argument type mismatch",
    );
}

// ---------------------------------------------------------------------- //
// Classes                                                                 //
// ---------------------------------------------------------------------- //

#[test]
fn a_child_class_inherits_its_parents_fields() {
    assert_lines(
        r#"
        class Animal { name: string }
        class Dog extends Animal { breed: string }
        let dog = Dog { name: "Buddy", breed: "Labrador" }
        println(dog.name)
        println(dog.breed)
        "#,
        &["Buddy", "Labrador"],
    );
}

#[test]
fn an_inherited_method_dispatches_to_the_concrete_override() {
    // `describe` is declared only on Animal, so the call inside it has to reach
    // the instance's own `speak` rather than Animal's.
    assert_lines(
        r#"
        class Animal {
            name: string
            fn speak(self) -> string { return "..." }
            fn describe(self) -> string { return self.name + " says " + self.speak() }
        }
        class Dog extends Animal {
            override fn speak(self) -> string { return "Woof" }
        }
        class Puppy extends Dog {
            override fn speak(self) -> string { return "Yip" }
        }
        println(Animal { name: "Generic" }.describe())
        println(Dog { name: "Rex" }.describe())
        println(Puppy { name: "Bella" }.describe())
        "#,
        &["Generic says ...", "Rex says Woof", "Bella says Yip"],
    );
}

#[test]
fn super_calls_the_parent_implementation_directly() {
    assert_lines(
        r#"
        class Animal {
            fn sound(this) -> string { return "generic" }
        }
        class Dog extends Animal {
            override fn sound(this) -> string { return "woof" }
            fn parent_sound(this) -> string { return super.sound() }
        }
        let d = Dog { }
        println(d.sound())
        println(d.parent_sound())
        "#,
        &["woof", "generic"],
    );
}

#[test]
fn this_and_self_name_the_same_receiver() {
    assert_lines(
        r#"
        class Animal {
            name: string
            fn who(this) -> string { return "this: " + this.name }
            fn also(self) -> string { return "self: " + self.name }
        }
        let a = Animal { name: "Rex" }
        println(a.who())
        println(a.also())
        "#,
        &["this: Rex", "self: Rex"],
    );
}

// ---------------------------------------------------------------------- //
// Generics                                                                //
// ---------------------------------------------------------------------- //

#[test]
fn a_generic_function_is_instantiated_per_argument_type() {
    assert_lines(
        r#"
        fn identity<T>(x: T) -> T { return x }
        println(identity(42))
        println(identity("hello"))
        println(identity(1.5))
        println(identity(true))
        "#,
        &["42", "hello", "1.5", "true"],
    );
}

#[test]
fn explicit_type_arguments_pick_the_instantiation() {
    assert_lines(
        r#"
        fn identity<T>(x: T) -> T { return x }
        println(identity::<int>(7))
        "#,
        &["7"],
    );
}

#[test]
fn a_generic_struct_takes_its_arguments_from_the_fields() {
    assert_lines(
        r#"
        struct Box<T> { value: T }
        impl<T> Box<T> {
            fn get(self) -> T { return self.value }
        }
        let ints = Box { value: 100 }
        let words = Box { value: "hello" }
        println(ints.value)
        println(words.value)
        println(ints.get())
        println(words.get())
        "#,
        &["100", "hello", "100", "hello"],
    );
}

#[test]
fn a_generic_enum_carries_a_typed_payload() {
    assert_lines(
        r#"
        enum Opt<T> {
            Some(T),
            None
        }
        enum Pair<A, B> {
            Both(A, B),
            Neither
        }
        fn describe(o: Opt<int>) -> string {
            return match o {
                Opt::Some(v) => "some " + v,
                Opt::None => "none"
            }
        }
        println(describe(Opt::Some(42)))
        println(describe(Opt::None))
        match Pair::Both(1, "hello") {
            Pair::Both(a, b) => println("both " + a + " " + b),
            Pair::Neither => println("neither")
        }
        "#,
        &["some 42", "none", "both 1 hello"],
    );
}

#[test]
fn one_instantiation_is_emitted_per_distinct_type() {
    // Two calls at the same type share a body; a third at another type gets its
    // own. Getting this wrong would either duplicate code or recurse forever.
    assert_lines(
        r#"
        fn first<T>(a: T, b: T) -> T { return a }
        println(first(1, 2))
        println(first(3, 4))
        println(first("x", "y"))
        "#,
        &["1", "3", "x"],
    );
}

#[test]
fn a_generic_method_infers_its_own_and_its_owners_type_arguments() {
    assert_lines(
        r#"
        struct Box<T> { value: T }
        impl<T> Box<T> {
            fn get(self) -> T { return self.value }
            fn map<U>(self, f: fn(T) -> U) -> Box<U> {
                return Box { value: f(self.value) }
            }
        }

        let number = Box { value: 7 }
        let text = number.map(|value: int| "n=" + value)
        println(number.get())
        println(text.get())
        "#,
        &["7", "n=7"],
    );
}

#[test]
fn a_generic_type_used_through_a_function_boundary_keeps_its_payload() {
    assert_lines(
        r#"
        enum Holder<T> { Value(T) }
        fn wrap(n: int) -> Holder<int> { return Holder::Value(n) }
        fn unwrap(h: Holder<int>) -> int {
            return match h {
                Holder::Value(v) => v
            }
        }
        println(unwrap(wrap(7)))
        "#,
        &["7"],
    );
}

#[test]
fn frontend_generic_examples_execute_natively() {
    assert_lines(
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/generics_basic.li"
        )),
        &[
            "=== Generic Functions ===",
            "identity int: 42",
            "identity string: hello",
            "=== Generic Structs ===",
            "box value: 100",
            "=== All Generics Tests Passed ===",
        ],
    );
    assert_lines(
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/generic_methods.li"
        )),
        &["got: 42", "mapped: 84", "name: lira"],
    );
    assert_lines(
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/generic_enum.li"
        )),
        &["42", "none", "both 1 hello", "neither", "5"],
    );
    assert_lines(
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/turbofish.li"
        )),
        &["5", "hello"],
    );
}

#[test]
fn generic_method_inference_seeds_owner_and_explicit_turbofish_appends_it() {
    assert_lines(
        r#"
        struct Box<T> { value: T }
        impl<T> Box<T> {
            fn echo<U>(self, value: U) -> U { return value }
        }
        fn make<T>(value: T) -> Box<T> { return Box { value: value } }
        let b = Box { value: 1 }
        println(b.echo("inferred"))
        println(b.echo::<string>("explicit"))
        println(make(2).echo("returned"))
        "#,
        &["inferred", "explicit", "returned"],
    );
}

#[test]
fn static_generic_method_calls_are_monomorphized() {
    assert_lines(
        r#"
        struct Factory {}
        impl Factory {
            fn make<T>(value: T) -> T { return value }
        }
        println(Factory.make::<string>("made"))
        "#,
        &["made"],
    );
}

#[test]
fn generic_recursion_keeps_distinct_concrete_instantiations() {
    assert_lines(
        r#"
        fn echo_after<T>(count: int, value: T) -> T {
            if count == 0 {
                return value
            }
            return echo_after(count - 1, value)
        }
        println(echo_after(2, 7))
        println(echo_after(1, "ok"))
        "#,
        &["7", "ok"],
    );
}

#[test]
fn generic_instantiations_run_through_the_jit_entry_point() {
    assert_jit_success(
        r#"
        fn identity<T>(value: T) -> T { return value }
        fn main() {
            if identity(42) != 42 { recv(chan(0)) }
            if identity("ok") != "ok" { recv(chan(0)) }
        }
        "#,
    );
}

#[test]
fn generic_aggregate_returns_are_concrete_at_field_access() {
    assert_lines(
        r#"
        struct Box<T> { value: T }
        struct Outer<T> { inner: Box<T> }
        impl<T> Box<T> {
            fn map<U>(self, value: U) -> Box<U> { return Box { value: value } }
        }
        fn make<T>(value: T) -> Box<T> { return Box { value: value } }
        struct Factory {}
        impl Factory {
            fn make<T>(value: T) -> Box<T> { return Box { value: value } }
        }
        println(Box { value: 7 }.value)
        println(Box { value: 8 }.map("mapped").value)
        println(Outer { inner: Box { value: 11 } }.inner.value)
        println(make(42).value)
        println(Factory.make(9).value)
        println(make("ok").value)
        "#,
        &["7", "mapped", "11", "42", "9", "ok"],
    );
}

#[test]
fn an_uninferrable_generic_parameter_has_a_native_diagnostic() {
    assert_rejected(
        r#"
        fn missing<T>() -> T { return 0 }
        println(missing())
        "#,
        "cannot work out the type arguments",
    );
}

// ---------------------------------------------------------------------- //
// Diagnostics                                                             //
// ---------------------------------------------------------------------- //

#[test]
fn dynamic_type_checks_run_through_the_native_any_representation() {
    assert_lines(
        "fn dynamic(x) { return x is int }\nprintln(dynamic(1))",
        &["true"],
    );
}

#[test]
fn a_type_error_stops_native_compilation() {
    assert_rejected("let x: int = \"not an int\"", "");
}

#[test]
fn malformed_any_source_still_reports_a_native_diagnostic() {
    assert_rejected("fn dynamic(x) { return x is int", "Expected '}'");
}

// ---------------------------------------------------------------------- //
// Object emission                                                         //
// ---------------------------------------------------------------------- //

#[test]
fn compile_object_produces_a_native_object_file() {
    let analysis = lirac::analyze("println(1)").expect("parses");
    let object = lira_codegen::aot::compile_object(&analysis.program, &analysis.sema)
        .expect("emits an object");
    assert!(!object.is_empty());
    if cfg!(target_os = "linux") {
        assert_eq!(&object[..4], b"\x7fELF");
    }
}

#[test]
fn the_output_binary_is_executable() {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let source_path = dir.join("program.li");
    let source = "println(\"hi\")";
    std::fs::write(&source_path, source).expect("write source");
    let output = common::run_aot(&source_path, source).expect("runs");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "hi\n");
    let _ = std::fs::remove_dir_all(&dir);
}
