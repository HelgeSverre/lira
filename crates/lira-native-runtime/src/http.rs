//! Blocking HTTP operations for the native Lira runtime.
//!
//! The exported functions use the same two-slot result representation as the
//! VM builtins: slot zero is an HTTP status code and slot one is a Lira string
//! containing the response body.  A transport, request-construction, or body
//! limit failure is represented by status `-1` and a descriptive error string.
//! Requests copy their arguments and run on the native I/O worker pool when
//! called from a fiber. Worker results remain opaque Rust-owned data until the
//! scheduler-thread completion callback materialises the public Lira array.

pub use crate::{LiraArray, LiraStr};
use std::ffi::{c_char, c_void};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::slice;
use std::str;
use std::time::Duration;

const LIRA_KIND_STRING: u32 = 1;
const MAX_ABI_BYTES: i64 = isize::MAX as i64 - 24;
const MAX_RESPONSE_BODY_BYTES: u64 = 10 * 1024 * 1024;
const GLOBAL_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

extern "C" {
    fn lira_rt_str_new(bytes: *const c_char, len: i64) -> *mut LiraStr;
    fn lira_rt_array_new(cap: i64) -> *mut LiraArray;
    fn lira_rt_array_push(array: *mut LiraArray, value: i64);
    fn lira_rt_panic(message: *const c_char);
    fn lira_rt_io_submit_current(
        work: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> i32,
        arg: *mut c_void,
        destroy_arg: unsafe extern "C" fn(*mut c_void),
        complete: unsafe extern "C" fn(*mut c_void, u64, *mut c_void, i32, *mut c_void),
        destroy_result: unsafe extern "C" fn(*mut c_void),
    ) -> i8;
    fn lira_rt_io_wake(owner: *mut c_void, generation: u64, status: i32) -> i8;
    fn lira_io_cancelled() -> i32;
    fn lira_io_test_fail_result_alloc(name: *const c_char) -> i32;
}

#[derive(Clone, Copy)]
enum RequestKind {
    Get,
    Post,
    Custom,
}

struct OwnedHttpRequest {
    kind: RequestKind,
    method: String,
    url: String,
    headers: String,
    body: String,
    content_type: String,
    result_slot: *mut *mut LiraArray,
}

struct OwnedHttpResult {
    result_slot: *mut *mut LiraArray,
    result: Result<(i64, String), String>,
}

/// Borrow a UTF-8 Lira string for the duration of one native call.
///
/// # Safety
///
/// The pointer must identify a live, correctly laid-out Lira string whose
/// trailing allocation contains `len` bytes.  This is guaranteed for values
/// produced by the generated runtime; malformed values are rejected before
/// slicing.
unsafe fn read_str<'a>(value: *const LiraStr) -> Option<&'a str> {
    if value.is_null() || !(value as usize).is_multiple_of(std::mem::align_of::<LiraStr>()) {
        return None;
    }
    let value_ref = &*value;
    if value_ref.hdr.kind != LIRA_KIND_STRING || !(0..=MAX_ABI_BYTES).contains(&value_ref.len) {
        return None;
    }
    let bytes = slice::from_raw_parts(value_ref.data.as_ptr(), value_ref.len as usize);
    str::from_utf8(bytes).ok()
}

unsafe fn new_string(value: &str) -> *mut LiraStr {
    lira_rt_str_new(value.as_ptr().cast::<c_char>(), value.len() as i64)
}

unsafe fn new_result(status: i64, body: &str) -> *mut LiraArray {
    let result = lira_rt_array_new(2);
    lira_rt_array_push(result, status);
    lira_rt_array_push(result, new_string(body).cast::<()>() as usize as i64);
    result
}

unsafe fn panic_runtime(message: &str) -> ! {
    let mut message_bytes = Vec::with_capacity(message.len() + 1);
    message_bytes.extend_from_slice(message.as_bytes());
    message_bytes.push(0);
    lira_rt_panic(message_bytes.as_ptr().cast::<c_char>());
    // The bundled C implementation terminates.  Aborting here also prevents
    // an embedding test implementation from allowing unwinding across FFI.
    std::process::abort()
}

fn invalid_abi() -> ! {
    unsafe { panic_runtime("invalid Lira string") }
}

fn ffi_call(operation: impl FnOnce() -> *mut LiraArray) -> *mut LiraArray {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(value) => value,
        Err(_) => unsafe { panic_runtime("HTTP runtime panic") },
    }
}

fn agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        // HTTP status codes, including 4xx and 5xx, are ordinary results.
        .http_status_as_error(false)
        .timeout_global(Some(GLOBAL_TIMEOUT))
        .timeout_resolve(Some(CONNECT_TIMEOUT))
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_send_request(Some(GLOBAL_TIMEOUT))
        .timeout_send_body(Some(GLOBAL_TIMEOUT))
        .timeout_recv_response(Some(GLOBAL_TIMEOUT))
        .timeout_recv_body(Some(GLOBAL_TIMEOUT))
        .build();
    ureq::Agent::new_with_config(config)
}

fn response_body(response: &mut ureq::http::Response<ureq::Body>) -> Result<String, String> {
    // Read at most one byte beyond the public limit. This makes an oversized
    // response observable without allowing unbounded allocation.
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BODY_BYTES + 1)
        .read_to_vec()
        .map_err(|error| format!("HTTP error: {error}"))?;
    if bytes.len() as u64 > MAX_RESPONSE_BODY_BYTES {
        return Err("HTTP response body exceeds 10 MiB limit".to_owned());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn send_request<B: ureq::AsSendBody>(
    request: ureq::http::Request<B>,
) -> Result<(i64, String), String> {
    let mut response = agent()
        .run(request)
        .map_err(|error| format!("HTTP error: {error}"))?;
    let status = i64::from(response.status().as_u16());
    // Match the VM: once a response status exists, a body read/decoding
    // failure produces an empty body instead of turning the request into a
    // transport error.  The configured reader limit still bounds allocation.
    let body = response_body(&mut response).unwrap_or_default();
    Ok((status, body))
}

fn get(url: &str) -> Result<(i64, String), String> {
    let request = ureq::http::Request::get(url)
        .body(())
        .map_err(|error| format!("Invalid request: {error}"))?;
    send_request(request)
}

fn post(url: &str, body: &str, content_type: &str) -> Result<(i64, String), String> {
    let request = ureq::http::Request::post(url)
        .header("Content-Type", content_type)
        .body(body.to_owned())
        .map_err(|error| format!("Invalid request: {error}"))?;
    send_request(request)
}

fn request(method: &str, url: &str, headers: &str, body: &str) -> Result<(i64, String), String> {
    let mut builder = ureq::http::Request::builder().method(method).uri(url);
    for line in headers.lines() {
        if let Some((name, value)) = line.split_once(':') {
            if let (Ok(name), Ok(value)) = (
                name.trim().parse::<ureq::http::HeaderName>(),
                value.trim().parse::<ureq::http::HeaderValue>(),
            ) {
                builder = builder.header(name, value);
            }
        }
    }
    let request = builder
        .body(body.to_owned())
        .map_err(|error| format!("Invalid request: {error}"))?;
    send_request(request)
}

fn result_from_call(call: Result<(i64, String), String>) -> *mut LiraArray {
    unsafe {
        match call {
            Ok((status, body)) => new_result(status, &body),
            Err(error) => new_result(-1, &error),
        }
    }
}

fn execute_owned(req: &OwnedHttpRequest) -> Result<(i64, String), String> {
    match req.kind {
        RequestKind::Get => get(&req.url),
        RequestKind::Post => post(&req.url, &req.body, &req.content_type),
        RequestKind::Custom => request(&req.method, &req.url, &req.headers, &req.body),
    }
}

unsafe extern "C" fn destroy_request(arg: *mut c_void) {
    drop(Box::from_raw(arg.cast::<OwnedHttpRequest>()));
}

unsafe extern "C" fn destroy_result(result: *mut c_void) {
    drop(Box::from_raw(result.cast::<OwnedHttpResult>()));
}

/// Worker entry point. It owns only Rust values copied by the scheduler
/// thread; it never calls into the Lira allocator or scheduler.
unsafe extern "C" fn http_work(arg: *mut c_void, out: *mut *mut c_void) -> i32 {
    let request = &*arg.cast::<OwnedHttpRequest>();
    if lira_io_cancelled() != 0 {
        *out = std::ptr::null_mut();
        return -1;
    }
    let call = catch_unwind(AssertUnwindSafe(|| execute_owned(request)));
    let result = match call {
        Ok(result) => result,
        Err(_) => Err("HTTP runtime panic".to_owned()),
    };
    if lira_io_cancelled() != 0 {
        *out = std::ptr::null_mut();
        return -1;
    }
    let fail_name = b"LIRA_TEST_FAIL_HTTP_RESULT_ALLOC\0";
    if lira_io_test_fail_result_alloc(fail_name.as_ptr().cast::<c_char>()) != 0 {
        *out = std::ptr::null_mut();
        return -1;
    }
    let owned = catch_unwind(AssertUnwindSafe(|| {
        Box::new(OwnedHttpResult {
            result_slot: request.result_slot,
            result,
        })
    }));
    match owned {
        Ok(owned) => {
            *out = Box::into_raw(owned).cast::<c_void>();
            0
        }
        Err(_) => {
            *out = std::ptr::null_mut();
            -1
        }
    }
}

/// Completion runs on the scheduler thread and is the only place an HTTP
/// result becomes a Lira array/string.
unsafe extern "C" fn http_complete(
    owner: *mut c_void,
    generation: u64,
    result: *mut c_void,
    status: i32,
    failure_arg: *mut c_void,
) {
    let completed = catch_unwind(AssertUnwindSafe(|| {
        if result.is_null() {
            if !failure_arg.is_null() {
                let request = &*failure_arg.cast::<OwnedHttpRequest>();
                request
                    .result_slot
                    .write(result_from_call(Err("HTTP worker failed".to_owned())));
            }
            lira_rt_io_wake(owner, generation, status);
            return;
        }
        let owned = &*result.cast::<OwnedHttpResult>();
        let value = if status == 0 {
            result_from_call(owned.result.clone())
        } else {
            result_from_call(Err("HTTP worker failed".to_owned()))
        };
        owned.result_slot.write(value);
        lira_rt_io_wake(owner, generation, 0);
    }));
    if completed.is_err() {
        panic_runtime("HTTP runtime panic");
    }
}

fn async_request(mut request: OwnedHttpRequest) -> *mut LiraArray {
    let mut result = std::ptr::null_mut();
    request.result_slot = &mut result;
    let raw = Box::into_raw(Box::new(request));
    let parked = unsafe {
        lira_rt_io_submit_current(
            http_work,
            raw.cast::<c_void>(),
            destroy_request,
            http_complete,
            destroy_result,
        )
    };
    match parked {
        1 => result,
        0 => {
            // Outside a fiber there is no scheduler to park. Reuse the same
            // owned request and execute synchronously for embedding callers.
            let request = unsafe { Box::from_raw(raw) };
            let value = execute_owned(&request);
            result_from_call(value)
        }
        _ => {
            unsafe { drop(Box::from_raw(raw)) };
            unsafe { panic_runtime("I/O worker pool is unavailable or full") }
        }
    }
}

/// Perform an HTTP GET and return `[status, body]`.
#[no_mangle]
pub extern "C" fn lira_rt_http_get(url: *const LiraStr) -> *mut LiraArray {
    ffi_call(|| unsafe {
        match read_str(url) {
            Some(url) => async_request(OwnedHttpRequest {
                kind: RequestKind::Get,
                method: String::new(),
                url: url.to_owned(),
                headers: String::new(),
                body: String::new(),
                content_type: String::new(),
                result_slot: std::ptr::null_mut(),
            }),
            None => invalid_abi(),
        }
    })
}

/// Perform an HTTP POST and return `[status, body]`.
#[no_mangle]
pub extern "C" fn lira_rt_http_post(
    url: *const LiraStr,
    body: *const LiraStr,
    content_type: *const LiraStr,
) -> *mut LiraArray {
    ffi_call(|| unsafe {
        match (read_str(url), read_str(body), read_str(content_type)) {
            (Some(url), Some(body), Some(content_type)) => async_request(OwnedHttpRequest {
                kind: RequestKind::Post,
                method: String::new(),
                url: url.to_owned(),
                headers: String::new(),
                body: body.to_owned(),
                content_type: content_type.to_owned(),
                result_slot: std::ptr::null_mut(),
            }),
            _ => invalid_abi(),
        }
    })
}

/// Perform a custom HTTP request and return `[status, body]`.
#[no_mangle]
pub extern "C" fn lira_rt_http_request(
    method: *const LiraStr,
    url: *const LiraStr,
    headers: *const LiraStr,
    body: *const LiraStr,
) -> *mut LiraArray {
    ffi_call(|| unsafe {
        match (
            read_str(method),
            read_str(url),
            read_str(headers),
            read_str(body),
        ) {
            (Some(method), Some(url), Some(headers), Some(body)) => {
                async_request(OwnedHttpRequest {
                    kind: RequestKind::Custom,
                    method: method.to_owned(),
                    url: url.to_owned(),
                    headers: headers.to_owned(),
                    body: body.to_owned(),
                    content_type: String::new(),
                    result_slot: std::ptr::null_mut(),
                })
            }
            _ => invalid_abi(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{get, post, request, MAX_RESPONSE_BODY_BYTES};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn serve(expected_request_fragment: &str, response: &'static str) -> String {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind test server");
        let address = listener.local_addr().expect("server address");
        let expected_request_fragment = expected_request_fragment.to_owned();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            loop {
                let read = stream.read(&mut chunk).expect("read request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let request_text = String::from_utf8_lossy(&request).to_lowercase();
            assert!(request_text.contains(&expected_request_fragment.to_lowercase()));
            stream
                .write_all(response.as_bytes())
                .expect("write response");
        });
        format!("http://{address}")
    }

    fn serve_exact_body(body_len: usize) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind body server");
        let address = listener.local_addr().expect("body server address");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept body request");
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            loop {
                let read = stream.read(&mut chunk).expect("read body request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {body_len}\r\nConnection: close\r\n\r\n"
            );
            stream
                .write_all(header.as_bytes())
                .expect("write body header");
            stream.write_all(&vec![b'a'; body_len]).expect("write body");
        });
        (format!("http://{address}"), handle)
    }

    #[test]
    fn get_preserves_non_success_status_and_body() {
        let url = serve(
            "GET ",
            "HTTP/1.1 404 Not Found\r\nContent-Length: 7\r\n\r\nmissing",
        );
        assert_eq!(
            get(&url).expect("GET response"),
            (404, "missing".to_owned())
        );
    }

    #[test]
    fn post_sets_content_type_and_returns_body() {
        let url = serve(
            "Content-Type: text/plain",
            "HTTP/1.1 201 Created\r\nContent-Length: 2\r\n\r\nok",
        );
        assert_eq!(post(&url, "x", "text/plain").expect("POST response").0, 201);
    }

    #[test]
    fn custom_request_ignores_malformed_header_lines_like_vm() {
        let url = serve("PATCH ", "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
        assert_eq!(
            request(
                "PATCH",
                &url,
                "not a header\n: invalid\nX-Test: yes",
                "body",
            )
            .expect("request"),
            (200, "ok".to_owned())
        );
    }

    #[test]
    fn response_body_at_exact_limit_is_accepted() {
        let (url, server) = serve_exact_body(MAX_RESPONSE_BODY_BYTES as usize);
        let (status, body) = get(&url).expect("maximum-sized response");
        assert_eq!(status, 200);
        assert_eq!(body.len(), MAX_RESPONSE_BODY_BYTES as usize);
        assert!(body.bytes().all(|byte| byte == b'a'));
        server.join().expect("maximum-sized body server completed");
    }
}
