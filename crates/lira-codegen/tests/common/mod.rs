//! Resource-bounded execution helpers for native integration tests.
//!
//! Generated programs are untrusted test inputs: a lowering bug can turn a
//! finite source loop into an infinite native loop or allocation storm. Keep
//! those failures inside a child process, bound their concurrency across
//! integration test binaries, and cap their lifetime and captured output.

#![allow(dead_code)]

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::os::unix::process::CommandExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_WALL_TIME: Duration = Duration::from_secs(20);
const DEFAULT_CPU_SECONDS: u64 = 15;
const GROUP_MEMORY_SNAPSHOT_RETRIES: usize = 4;
const INSPECTION_EXIT_GRACE: Duration = Duration::from_millis(20);
const CLEANUP_WAIT_LIMIT: Duration = Duration::from_millis(250);
const ADDRESS_SPACE_LIMIT_BYTES: u64 = 768 * 1024 * 1024;
const CHILD_MEMORY_LIMIT_BYTES: u64 = 768 * 1024 * 1024;
const OUTPUT_LIMIT_BYTES: usize = 1024 * 1024;
pub const SOURCE_LIMIT_BYTES: usize = 8 * 1024 * 1024;
const MAX_JIT_PROGRAMS: usize = 256;
const MAX_JIT_SEQUENCE_SOURCE_BYTES: usize = SOURCE_LIMIT_BYTES;
const FRONTEND_FRAME_LIMIT_BYTES: usize = OUTPUT_LIMIT_BYTES;
const FRONTEND_DIAGNOSTIC_LIMIT: usize = 4096;
const FRONTEND_MESSAGE_LIMIT_BYTES: usize = 64 * 1024;
const NATIVE_MEMORY_LIMIT_BYTES: &str = "268435456";
const NATIVE_MAX_FIBERS: &str = "512";
const JIT_CHILD_ENV: &str = "LIRA_TEST_BOUNDED_JIT_CHILD";
const JIT_SOURCE_ENV: &str = "LIRA_TEST_BOUNDED_JIT_SOURCE";
const JIT_SOURCE_NAME_ENV: &str = "LIRA_TEST_BOUNDED_JIT_SOURCE_NAME";
const JIT_RESULT_ENV: &str = "LIRA_TEST_BOUNDED_JIT_RESULT";
const JIT_STDOUT_ENV: &str = "LIRA_TEST_BOUNDED_JIT_STDOUT";
const JIT_SEQUENCE_DIR_ENV: &str = "LIRA_TEST_BOUNDED_JIT_SEQUENCE_DIR";
const JIT_SEQUENCE_COUNT_ENV: &str = "LIRA_TEST_BOUNDED_JIT_SEQUENCE_COUNT";
const AOT_CHILD_ENV: &str = "LIRA_TEST_BOUNDED_AOT_CHILD";
const AOT_SOURCE_ENV: &str = "LIRA_TEST_BOUNDED_AOT_SOURCE";
const AOT_SOURCE_NAME_ENV: &str = "LIRA_TEST_BOUNDED_AOT_SOURCE_NAME";
const AOT_RESULT_ENV: &str = "LIRA_TEST_BOUNDED_AOT_RESULT";
const AOT_BINARY_ENV: &str = "LIRA_TEST_BOUNDED_AOT_BINARY";
const AOT_BUILD_ONLY_ENV: &str = "LIRA_TEST_BOUNDED_AOT_BUILD_ONLY";
const VM_CHILD_ENV: &str = "LIRA_TEST_BOUNDED_VM_CHILD";
const VM_SOURCE_ENV: &str = "LIRA_TEST_BOUNDED_VM_SOURCE";
const VM_SOURCE_NAME_ENV: &str = "LIRA_TEST_BOUNDED_VM_SOURCE_NAME";
const VM_RESULT_ENV: &str = "LIRA_TEST_BOUNDED_VM_RESULT";
const VM_STDOUT_ENV: &str = "LIRA_TEST_BOUNDED_VM_STDOUT";
const FRONTEND_CHILD_ENV: &str = "LIRA_TEST_BOUNDED_FRONTEND_CHILD";
const FRONTEND_SOURCE_ENV: &str = "LIRA_TEST_BOUNDED_FRONTEND_SOURCE";
const FRONTEND_SOURCE_NAME_ENV: &str = "LIRA_TEST_BOUNDED_FRONTEND_SOURCE_NAME";
const FRONTEND_RESULT_ENV: &str = "LIRA_TEST_BOUNDED_FRONTEND_RESULT";

const VM_RESERVED_ENVS: &[&str] = &[
    VM_CHILD_ENV,
    VM_SOURCE_ENV,
    VM_SOURCE_NAME_ENV,
    VM_RESULT_ENV,
    VM_STDOUT_ENV,
    "LIRA_NATIVE_MEMORY_LIMIT_BYTES",
    "LIRA_NATIVE_MAX_FIBERS",
];
const AOT_RESERVED_ENVS: &[&str] = &[
    AOT_CHILD_ENV,
    AOT_SOURCE_ENV,
    AOT_SOURCE_NAME_ENV,
    AOT_RESULT_ENV,
    AOT_BINARY_ENV,
    AOT_BUILD_ONLY_ENV,
    "LIRA_NATIVE_MEMORY_LIMIT_BYTES",
    "LIRA_NATIVE_MAX_FIBERS",
];
const JIT_RESERVED_ENVS: &[&str] = &[
    JIT_CHILD_ENV,
    JIT_SOURCE_ENV,
    JIT_SOURCE_NAME_ENV,
    JIT_RESULT_ENV,
    JIT_STDOUT_ENV,
    JIT_SEQUENCE_DIR_ENV,
    JIT_SEQUENCE_COUNT_ENV,
    "LIRA_NATIVE_MEMORY_LIMIT_BYTES",
    "LIRA_NATIVE_MAX_FIBERS",
];

static NEXT_SCRATCH_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub struct LocalCrawlerReport {
    pub paths: Vec<String>,
    pub unknown_paths: Vec<String>,
    pub error: Option<String>,
}

pub struct LocalCrawlerServer {
    pub base_url: String,
    report_rx: Receiver<LocalCrawlerReport>,
    join: Option<thread::JoinHandle<()>>,
}

impl LocalCrawlerServer {
    pub fn start(expected_requests: usize) -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        let (report_tx, report_rx) = mpsc::channel();
        let paths = Arc::new(Mutex::new(Vec::new()));
        let unknown_paths = Arc::new(Mutex::new(Vec::new()));
        let join = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut error = None;
            loop {
                let path_count = paths
                    .lock()
                    .map(|paths| paths.len())
                    .unwrap_or(expected_requests);
                if path_count >= expected_requests {
                    break;
                }
                if Instant::now() >= deadline {
                    error = Some(format!(
                        "local crawler server timed out after {} of {} requests",
                        path_count, expected_requests
                    ));
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        if let Err(connection_error) =
                            handle_local_crawler_connection(stream, &paths, &unknown_paths)
                        {
                            error = Some(connection_error);
                            break;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(accept_error) => {
                        error = Some(format!("local crawler accept failed: {accept_error}"));
                        break;
                    }
                }
            }
            let report = LocalCrawlerReport {
                paths: paths.lock().map(|paths| paths.clone()).unwrap_or_default(),
                unknown_paths: unknown_paths
                    .lock()
                    .map(|paths| paths.clone())
                    .unwrap_or_default(),
                error,
            };
            let _ = report_tx.send(report);
        });
        Ok(Self {
            base_url: format!("http://127.0.0.1:{port}"),
            report_rx,
            join: Some(join),
        })
    }

    pub fn finish(mut self) -> Result<LocalCrawlerReport, String> {
        let report = self
            .report_rx
            .recv_timeout(Duration::from_secs(6))
            .map_err(|error| format!("local crawler server did not finish: {error}"))?;
        if let Some(join) = self.join.take() {
            join.join()
                .map_err(|_| "local crawler server panicked".to_string())?;
        }
        Ok(report)
    }
}

impl Drop for LocalCrawlerServer {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn handle_local_crawler_connection(
    mut stream: TcpStream,
    paths: &Arc<Mutex<Vec<String>>>,
    unknown_paths: &Arc<Mutex<Vec<String>>>,
) -> Result<(), String> {
    stream
        .set_nonblocking(false)
        .map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    let mut request = Vec::new();
    let mut chunk = [0_u8; 1024];
    while request.len() < 8192 && !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        let remaining = 8192 - request.len();
        request.extend_from_slice(&chunk[..count.min(remaining)]);
    }
    let request = String::from_utf8_lossy(&request);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .map(|target| target.split('?').next().unwrap_or(target).to_string())
        .ok_or_else(|| "local crawler received malformed HTTP request".to_string())?;
    paths
        .lock()
        .map_err(|_| "local crawler path lock poisoned".to_string())?
        .push(path.clone());
    let (status, body) = match path.as_str() {
        "/" => (
            "200 OK",
            "<a href=\"/page/1\">1</a><a href=\"/page/2\">2</a><a href=\"/page/3\">3</a><a href=\"http://external.invalid/\">external</a><a href=\"/static/site.css\">asset</a><a href=\"#fragment\">fragment</a>",
        ),
        "/page/1" | "/page/2" | "/page/3" => (
            "200 OK",
            "<a href=\"/\">root</a><a href=\"/page/1\">1</a><a href=\"/page/2\">2</a><a href=\"/page/3\">3</a>",
        ),
        other => {
            unknown_paths
                .lock()
                .map_err(|_| "local crawler unknown-path lock poisoned".to_string())?
                .push(other.to_string());
            ("404 Not Found", "unknown")
        }
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|error| error.to_string())
}

struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    fn new(label: &str) -> io::Result<Self> {
        loop {
            let path = scratch_dir(label);
            match std::fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[derive(Debug)]
pub struct BoundedOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// Time spent in the child after it acquired the global execution lock.
    pub elapsed: Duration,
    pub timed_out: bool,
    pub memory_exceeded: bool,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

impl BoundedOutput {
    pub fn stdout_text(&self) -> String {
        String::from_utf8(self.stdout.clone()).expect("bounded child stdout must be UTF-8")
    }

    pub fn stderr_text(&self) -> String {
        String::from_utf8(self.stderr.clone()).expect("bounded child stderr must be UTF-8")
    }

    pub fn assert_complete_output(&self) -> Result<(), String> {
        if self.stdout_truncated || self.stderr_truncated {
            return Err(format!(
                "bounded child exceeded the {} byte output limit (stdout truncated: {}, stderr truncated: {})",
                OUTPUT_LIMIT_BYTES, self.stdout_truncated, self.stderr_truncated
            ));
        }
        std::str::from_utf8(&self.stdout)
            .map_err(|error| format!("bounded child stdout is not valid UTF-8: {error}"))?;
        std::str::from_utf8(&self.stderr)
            .map_err(|error| format!("bounded child stderr is not valid UTF-8: {error}"))?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum VmRunOutcome {
    Success { status: i32, output: Vec<u8> },
    CompileError(String),
    RuntimeError { message: String, output: Vec<u8> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontendDiagnostic {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrontendPreflightOutcome {
    Accepted,
    CompileError(Vec<FrontendDiagnostic>),
}

/// One held slot in the global bounded-concurrency pool. Owning a lock means
/// one bounded child may execute; the lock is released when the child's whole
/// lifecycle (compile, link, run, harvest) has finished.
const DEFAULT_EXECUTION_LANES: usize = 4;
const MAX_EXECUTION_LANES: usize = 16;

static EXECUTION_LANES: OnceLock<usize> = OnceLock::new();
static NEXT_LANE_PICK: AtomicU64 = AtomicU64::new(0);

/// Number of bounded child executions that may run at once, across every test
/// thread and every integration test binary. Historically this was exactly one
/// (the lock fully serialized native work). More than one lane lets independent
/// examples run in parallel on the many-core test machines while the cap still
/// bounds concurrent Cranelift compiles, linker runs, and generated programs,
/// so a lowering bug cannot starve the machine of memory. Override with
/// `LIRA_TEST_EXEC_LANES` (clamped to 1..=16).
pub fn execution_lane_count() -> usize {
    *EXECUTION_LANES.get_or_init(|| {
        std::env::var("LIRA_TEST_EXEC_LANES")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(DEFAULT_EXECUTION_LANES)
            .clamp(1, MAX_EXECUTION_LANES)
    })
}

struct ExecutionLock(File);

impl ExecutionLock {
    fn acquire() -> io::Result<Self> {
        let lanes = execution_lane_count();
        // Vary the probe start so many parked test threads spread across the
        // lanes instead of stampeding the same first lane.
        let start = NEXT_LANE_PICK.fetch_add(1, Ordering::Relaxed) % lanes as u64;

        // Nonblocking acquisition lets a stale external runner fail within a
        // bounded time instead of hanging the test process forever. Each lane
        // is an exclusive flock on its own file, so up to `lanes` fds across
        // the machine hold an exclusive lock at once. flock(2) treats fds to
        // the same file independently, so these files also arbitrate between
        // threads of one test process.
        let deadline = Instant::now() + Duration::from_secs(600);
        loop {
            for offset in 0..lanes {
                let lane = (start + offset as u64) % lanes as u64;
                let path = std::env::temp_dir().join(format!("lira-bounded-exec-lane-{lane}"));
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .mode(0o600)
                    .open(&path)?;
                // SAFETY: `file` owns this valid descriptor until `ExecutionLock`
                // is dropped.
                let result =
                    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
                if result == 0 {
                    return Ok(Self(file));
                }
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted
                    && !matches!(
                        error.raw_os_error(),
                        Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN
                    )
                {
                    return Err(error);
                }
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "timed out waiting for a global native execution lane",
                ));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for ExecutionLock {
    fn drop(&mut self) {
        // SAFETY: the descriptor remains valid for the duration of this call.
        let _ = unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

fn apply_child_limits(command: &mut Command) {
    let has_memory_limit = command
        .get_envs()
        .any(|(name, _)| name == std::ffi::OsStr::new("LIRA_NATIVE_MEMORY_LIMIT_BYTES"));
    let has_fiber_limit = command
        .get_envs()
        .any(|(name, _)| name == std::ffi::OsStr::new("LIRA_NATIVE_MAX_FIBERS"));
    if !has_memory_limit {
        command.env("LIRA_NATIVE_MEMORY_LIMIT_BYTES", NATIVE_MEMORY_LIMIT_BYTES);
    }
    if !has_fiber_limit {
        command.env("LIRA_NATIVE_MAX_FIBERS", NATIVE_MAX_FIBERS);
    }
    // SAFETY: this closure runs after fork and before exec. It only invokes
    // async-signal-safe `setrlimit` calls and constructs errors from errno.
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            let cpu = libc::rlimit {
                rlim_cur: DEFAULT_CPU_SECONDS as libc::rlim_t,
                rlim_max: (DEFAULT_CPU_SECONDS + 1) as libc::rlim_t,
            };
            if libc::setrlimit(libc::RLIMIT_CPU, &cpu) != 0 {
                return Err(io::Error::last_os_error());
            }

            #[cfg(target_os = "linux")]
            {
                let address_space = libc::rlimit {
                    rlim_cur: ADDRESS_SPACE_LIMIT_BYTES as libc::rlim_t,
                    rlim_max: ADDRESS_SPACE_LIMIT_BYTES as libc::rlim_t,
                };
                if libc::setrlimit(libc::RLIMIT_AS, &address_space) != 0 {
                    return Err(io::Error::last_os_error());
                }
            }

            Ok(())
        });
    }
}

fn validate_child_envs(envs: &[(&str, &str)], reserved: &[&str]) -> Result<(), String> {
    if let Some((name, _)) = envs
        .iter()
        .find(|(name, _)| reserved.iter().any(|reserved| reserved == name))
    {
        return Err(format!(
            "caller environment variable `{name}` is reserved by the bounded child protocol"
        ));
    }
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn child_memory_bytes(pid: u32) -> io::Result<Option<u64>> {
    retry_complete_group_snapshot(GROUP_MEMORY_SNAPSHOT_RETRIES, || {
        child_memory_bytes_once(pid)
    })
}

/// Retry only snapshots invalidated by a member exiting or its proc entry
/// disappearing after enumeration.
/// Every retry reruns the full enumeration and summation, so the monitor never
/// accepts a partial process-group total.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn retry_complete_group_snapshot<T>(
    attempts: usize,
    mut snapshot: impl FnMut() -> io::Result<T>,
) -> io::Result<T> {
    debug_assert!(attempts > 0);
    for attempt in 0..attempts {
        match snapshot() {
            Ok(value) => return Ok(value),
            Err(error) if is_transient_group_snapshot_error(&error) && attempt + 1 < attempts => {
                thread::yield_now();
            }
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other(
        "process-group memory sampling retry count was zero",
    ))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn is_transient_group_snapshot_error(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound || error.raw_os_error() == Some(libc::ESRCH)
}

#[cfg(target_os = "macos")]
fn child_memory_bytes_once(pid: u32) -> io::Result<Option<u64>> {
    let pid_size = std::mem::size_of::<libc::pid_t>();
    let mut capacity = 32_usize;
    let pids = loop {
        let mut pids = vec![0; capacity];
        let buffer_bytes = capacity
            .checked_mul(pid_size)
            .ok_or_else(|| io::Error::other("bounded process-group enumeration overflowed"))?;
        let buffer_bytes = libc::c_int::try_from(buffer_bytes)
            .map_err(|_| io::Error::other("bounded process-group enumeration is too large"))?;
        let count = unsafe {
            libc::proc_listpgrppids(pid as libc::pid_t, pids.as_mut_ptr().cast(), buffer_bytes)
        };
        if count < 0 {
            return Err(io::Error::last_os_error());
        }
        // Unlike `proc_listpids`, the convenience wrapper
        // `proc_listpgrppids` returns a PID count, not a byte count.
        let count = count as usize;
        if count > capacity {
            return Err(io::Error::other(
                "bounded process-group enumeration exceeded its buffer",
            ));
        }
        if count < capacity {
            pids.truncate(count);
            break pids;
        }
        capacity = capacity
            .checked_mul(2)
            .ok_or_else(|| io::Error::other("bounded process-group enumeration overflowed"))?;
        if capacity > 4096 {
            return Err(io::Error::other(
                "bounded child process group has too many members",
            ));
        }
    };
    if pids.is_empty() {
        return Err(io::Error::other(
            "bounded child process group has no inspectable members",
        ));
    }
    let mut total = 0_u64;
    for child_pid in pids {
        if child_pid <= 0 {
            return Err(io::Error::other(
                "bounded child process group contains an invalid PID",
            ));
        }
        let mut usage = std::mem::MaybeUninit::<libc::rusage_info_v2>::zeroed();
        // SAFETY: `usage` points to writable storage of the exact structure
        // named by RUSAGE_INFO_V2. Membership came from the current snapshot;
        // an exit before this call is reported as ESRCH and causes the caller
        // to restart the complete snapshot rather than accept a partial sum.
        let result = unsafe {
            libc::proc_pid_rusage(child_pid, libc::RUSAGE_INFO_V2, usage.as_mut_ptr().cast())
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: proc_pid_rusage returned success and initialized the
        // structure.
        total = total.saturating_add(unsafe { usage.assume_init() }.ri_phys_footprint);
    }
    Ok(Some(total))
}

#[cfg(target_os = "linux")]
fn child_memory_bytes_once(pid: u32) -> io::Result<Option<u64>> {
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page_size <= 0 {
        return Err(io::Error::other("could not determine Linux page size"));
    }
    let mut total_pages = 0_u64;
    let mut members = 0_u64;
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let member_pid = match name.parse::<u32>() {
            Ok(member_pid) => member_pid,
            Err(_) => continue,
        };
        let stat_path = entry.path().join("stat");
        let stat = std::fs::read_to_string(stat_path)?;
        // `comm` is parenthesized and may itself contain spaces and `)`.
        // Searching from the right leaves the stable fields after it intact.
        let Some(comm_end) = stat.rfind(')') else {
            return Err(io::Error::other(format!(
                "malformed /proc/{member_pid}/stat: missing comm terminator"
            )));
        };
        let fields = stat[comm_end + 1..].split_whitespace().collect::<Vec<_>>();
        if fields.len() <= 21 {
            return Err(io::Error::other(format!(
                "malformed /proc/{member_pid}/stat: missing RSS field"
            )));
        }
        let process_group = fields[2].parse::<u32>().map_err(|error| {
            io::Error::other(format!(
                "invalid process group in /proc/{member_pid}/stat: {error}"
            ))
        })?;
        if process_group != pid {
            continue;
        }
        let rss_pages = fields[21].parse::<u64>().map_err(|error| {
            io::Error::other(format!("invalid RSS in /proc/{member_pid}/stat: {error}"))
        })?;
        members += 1;
        total_pages = total_pages.saturating_add(rss_pages);
    }
    if members == 0 {
        return Err(io::Error::other(
            "bounded child process group has no inspectable members",
        ));
    }
    Ok(Some(total_pages.saturating_mul(page_size as u64)))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn child_memory_bytes(_pid: u32) -> io::Result<Option<u64>> {
    Ok(None)
}

fn kill_child_group(child: &mut std::process::Child) -> io::Result<ExitStatus> {
    // SAFETY: the child called `setpgid(0, 0)` before exec, making its PID the
    // process-group id. ESRCH simply means it exited before the signal.
    let mut group_error = None;
    if unsafe { libc::killpg(child.id() as libc::pid_t, libc::SIGKILL) } != 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            group_error = Some(error);
        }
        if let Err(error) = child.kill() {
            if error.raw_os_error() != Some(libc::ESRCH) {
                group_error.get_or_insert(error);
            }
        }
    }
    let wait_result = reap_child_bounded(child);
    match (wait_result, group_error) {
        (Ok(status), None) => Ok(status),
        (Ok(status), Some(error)) => Err(io::Error::other(format!(
            "could not kill bounded child process group (child status {status}): {error}"
        ))),
        (Err(wait_error), None) => Err(wait_error),
        (Err(wait_error), Some(group_error)) => Err(io::Error::other(format!(
            "could not kill bounded child process group: {group_error}; \
             could not reap bounded child: {wait_error}"
        ))),
    }
}

fn kill_child_only(child: &mut std::process::Child) -> io::Result<ExitStatus> {
    let kill_error = match child.kill() {
        Ok(()) => None,
        Err(error) if error.raw_os_error() == Some(libc::ESRCH) => None,
        Err(error) => Some(error),
    };
    let status = reap_child_bounded(child)?;
    if let Some(error) = kill_error {
        return Err(io::Error::other(format!(
            "could not kill generated executable (status {status}): {error}"
        )));
    }
    Ok(status)
}

fn reap_completed_child_group(child: &mut std::process::Child) -> io::Result<ExitStatus> {
    // WNOWAIT left the completed leader unreaped, so its PID still identifies
    // this process group. Kill any descendants before allowing PID reuse.
    let group_error = if unsafe { libc::killpg(child.id() as libc::pid_t, libc::SIGKILL) } != 0 {
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::ESRCH) => None,
            Some(libc::EPERM) if completed_group_has_no_descendants(child.id() as libc::pid_t)? => {
                None
            }
            _ => Some(error),
        }
    } else {
        None
    };
    let wait_result = reap_child_bounded(child);
    match (wait_result, group_error) {
        (Ok(status), None) => Ok(status),
        (Ok(status), Some(error)) => Err(io::Error::other(format!(
            "could not clean up completed child process group (child status {status}): {error}"
        ))),
        (Err(error), None) => Err(error),
        (Err(wait_error), Some(group_error)) => Err(io::Error::other(format!(
            "could not clean up completed child process group: {group_error}; \
             could not reap completed child: {wait_error}"
        ))),
    }
}

#[cfg(target_os = "macos")]
fn completed_group_has_no_descendants(group_id: libc::pid_t) -> io::Result<bool> {
    let pid_size = std::mem::size_of::<libc::pid_t>();
    let mut capacity = 32_usize;
    loop {
        let mut pids = vec![0 as libc::pid_t; capacity];
        let buffer_bytes = capacity
            .checked_mul(pid_size)
            .ok_or_else(|| io::Error::other("completed process-group enumeration overflowed"))?;
        let buffer_bytes = libc::c_int::try_from(buffer_bytes)
            .map_err(|_| io::Error::other("completed process-group enumeration is too large"))?;
        let count =
            unsafe { libc::proc_listpgrppids(group_id, pids.as_mut_ptr().cast(), buffer_bytes) };
        if count < 0 {
            return Err(io::Error::last_os_error());
        }
        let count = count as usize;
        if count > capacity {
            return Err(io::Error::other(
                "completed process-group enumeration exceeded its buffer",
            ));
        }
        if count < capacity {
            pids.truncate(count);
            return Ok(pids.into_iter().all(|pid| pid <= 0 || pid == group_id));
        }
        capacity = capacity
            .checked_mul(2)
            .ok_or_else(|| io::Error::other("completed process-group enumeration overflowed"))?;
        if capacity > 4096 {
            return Err(io::Error::other(
                "completed process group has too many members",
            ));
        }
    }
}

#[cfg(target_os = "linux")]
fn completed_group_has_no_descendants(group_id: libc::pid_t) -> io::Result<bool> {
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let member_pid = match name.parse::<libc::pid_t>() {
            Ok(pid) => pid,
            Err(_) => continue,
        };
        let stat = match std::fs::read_to_string(entry.path().join("stat")) {
            Ok(stat) => stat,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let Some(comm_end) = stat.rfind(')') else {
            return Err(io::Error::other(format!(
                "malformed /proc/{member_pid}/stat: missing comm terminator"
            )));
        };
        let fields = stat[comm_end + 1..].split_whitespace().collect::<Vec<_>>();
        if fields.len() <= 2 {
            return Err(io::Error::other(format!(
                "malformed /proc/{member_pid}/stat: missing process group"
            )));
        }
        let member_group = fields[2].parse::<libc::pid_t>().map_err(|error| {
            io::Error::other(format!(
                "invalid process group in /proc/{member_pid}/stat: {error}"
            ))
        })?;
        if member_group == group_id && member_pid != group_id && fields[0] != "Z" {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn completed_group_has_no_descendants(_group_id: libc::pid_t) -> io::Result<bool> {
    Ok(false)
}

fn reap_child_bounded(child: &mut std::process::Child) -> io::Result<ExitStatus> {
    let deadline = Instant::now() + CLEANUP_WAIT_LIMIT;
    loop {
        if child_exit_pending(child)? {
            return child.wait();
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "timed out waiting to reap bounded child",
            ));
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn output_file_sizes(stdout_path: &Path, stderr_path: &Path) -> io::Result<(u64, u64)> {
    let stdout = std::fs::metadata(stdout_path)?.len();
    let stderr = std::fs::metadata(stderr_path)?.len();
    Ok((stdout, stderr))
}

fn output_limit_flags(stdout: u64, stderr: u64) -> (bool, bool) {
    let mut stdout_truncated = stdout > OUTPUT_LIMIT_BYTES as u64;
    let mut stderr_truncated = stderr > OUTPUT_LIMIT_BYTES as u64;
    if stdout.saturating_add(stderr) > OUTPUT_LIMIT_BYTES as u64 {
        if stdout > 0 {
            stdout_truncated = true;
        } else {
            stderr_truncated = true;
        }
    }
    (stdout_truncated, stderr_truncated)
}

fn status_after_inspection_race(child: &mut std::process::Child) -> io::Result<Option<ExitStatus>> {
    let deadline = Instant::now() + INSPECTION_EXIT_GRACE;
    loop {
        if child_exit_pending(child)? {
            return reap_completed_child_group(child).map(Some);
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        // macOS can remove a just-exited process from `proc_listpgrppids`
        // before `waitpid(WNOHANG)` exposes its status to the parent. Keep
        // this grace short: a genuinely uninspectable live child remains a
        // containment failure and is killed by the caller.
        thread::sleep(Duration::from_millis(1));
    }
}

fn child_exit_pending(child: &std::process::Child) -> io::Result<bool> {
    loop {
        let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
        // SAFETY: `info` is valid output storage. WNOWAIT observes this direct
        // child without reaping it, keeping its PID/group identity stable until
        // `reap_completed_child_group` has killed every descendant.
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                child.id() as libc::id_t,
                info.as_mut_ptr(),
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result == 0 {
            // SAFETY: successful waitid initialized the structure. WNOHANG
            // reports no pending status with a zero si_pid.
            return Ok(unsafe { info.assume_init().si_pid() } != 0);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

fn apply_output_file_limit(command: &mut Command) {
    // This is installed only on the generated executable, never on the AOT
    // compiler wrapper (which must create larger object and binary files).
    unsafe {
        command.pre_exec(|| {
            let file_size = libc::rlimit {
                rlim_cur: OUTPUT_LIMIT_BYTES as libc::rlim_t,
                rlim_max: OUTPUT_LIMIT_BYTES as libc::rlim_t,
            };
            if libc::setrlimit(libc::RLIMIT_FSIZE, &file_size) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

pub fn run_command(command: &mut Command) -> io::Result<BoundedOutput> {
    run_command_with_wall_time(command, DEFAULT_WALL_TIME)
}

pub fn run_command_with_wall_time(
    command: &mut Command,
    wall_time: Duration,
) -> io::Result<BoundedOutput> {
    let _execution_lock = ExecutionLock::acquire()?;
    let scratch = ScratchDir::new("command")?;
    let dir = scratch.path();
    let stdout_path = dir.join("stdout");
    let stderr_path = dir.join("stderr");
    command.stdout(Stdio::from(File::create(&stdout_path)?));
    command.stderr(Stdio::from(File::create(&stderr_path)?));
    apply_child_limits(command);
    let mut child = command.spawn()?;
    let started = Instant::now();
    let (outcome, mut stdout_truncated, mut stderr_truncated) = loop {
        match child_exit_pending(&child) {
            Ok(true) => {
                break (
                    reap_completed_child_group(&mut child).map(|status| (status, false, false)),
                    false,
                    false,
                )
            }
            Ok(false) => {}
            Err(error) => {
                let error = match kill_child_group(&mut child) {
                    Ok(_) => error,
                    Err(kill_error) => io::Error::other(format!(
                        "could not reap bounded child after wait failure ({error}): {kill_error}"
                    )),
                };
                break (Err(error), false, false);
            }
        }
        if started.elapsed() >= wall_time {
            // Kill the complete child process group so a generated program
            // cannot leave helper descendants behind after its deadline.
            break (
                kill_child_group(&mut child).map(|status| (status, true, false)),
                false,
                false,
            );
        }
        match child_memory_bytes(child.id()) {
            Ok(Some(bytes)) if bytes > CHILD_MEMORY_LIMIT_BYTES => {
                break (
                    kill_child_group(&mut child).map(|status| (status, false, true)),
                    false,
                    false,
                );
            }
            Ok(_) => {}
            Err(error) => match status_after_inspection_race(&mut child) {
                // A process can exit between the first status peek and the
                // group inspection. That is normal completion, not a
                // monitoring failure.
                Ok(Some(status)) => break (Ok((status, false, false)), false, false),
                Ok(None) => {
                    let containment_error = match kill_child_group(&mut child) {
                        Ok(_) => io::Error::other(format!(
                            "could not inspect bounded child process group memory: {error}"
                        )),
                        Err(kill_error) => io::Error::other(format!(
                            "could not inspect bounded child process group memory: {error}; \
                             could not kill group: {kill_error}"
                        )),
                    };
                    break (Err(containment_error), false, false);
                }
                Err(wait_error) => {
                    let error = match kill_child_group(&mut child) {
                        Ok(_) => wait_error,
                        Err(kill_error) => io::Error::other(format!(
                            "could not recheck bounded child after memory inspection failure \
                             ({wait_error}): {kill_error}"
                        )),
                    };
                    break (Err(error), false, false);
                }
            },
        }
        let (stdout_len, stderr_len) = match output_file_sizes(&stdout_path, &stderr_path) {
            Ok(sizes) => sizes,
            Err(error) => {
                let error = match kill_child_group(&mut child) {
                    Ok(_) => io::Error::other(format!(
                        "could not inspect bounded child output files: {error}"
                    )),
                    Err(kill_error) => io::Error::other(format!(
                        "could not inspect bounded child output files: {error}; \
                         could not kill group: {kill_error}"
                    )),
                };
                break (Err(error), false, false);
            }
        };
        let (current_stdout_truncated, current_stderr_truncated) =
            output_limit_flags(stdout_len, stderr_len);
        if current_stdout_truncated || current_stderr_truncated {
            break (
                kill_child_group(&mut child).map(|status| (status, false, false)),
                current_stdout_truncated,
                current_stderr_truncated,
            );
        }
        thread::sleep(Duration::from_millis(10));
    };
    let (status, timed_out, memory_exceeded) = outcome?;
    let (final_stdout_len, final_stderr_len) = output_file_sizes(&stdout_path, &stderr_path)?;
    let (final_stdout_truncated, final_stderr_truncated) =
        output_limit_flags(final_stdout_len, final_stderr_len);
    stdout_truncated |= final_stdout_truncated;
    stderr_truncated |= final_stderr_truncated;
    let elapsed = started.elapsed();
    let (stdout, stdout_file_truncated) = read_bounded_file(&stdout_path)?;
    let (stderr, stderr_file_truncated) = read_bounded_file(&stderr_path)?;
    stdout_truncated |= stdout_file_truncated;
    stderr_truncated |= stderr_file_truncated;
    Ok(BoundedOutput {
        status,
        stdout,
        stderr,
        elapsed,
        timed_out,
        memory_exceeded,
        stdout_truncated,
        stderr_truncated,
    })
}

/// Compile and execute bytecode in an isolated, resource-bounded child.
/// Compiler and language-runtime failures remain distinct from containment
/// failures so an expected program error cannot accidentally accept a timeout,
/// output overflow, or memory-limit breach.
pub fn run_vm_capture(source_file: &Path, source: &str) -> Result<VmRunOutcome, String> {
    run_vm_capture_with_envs(source_file, source, &[], DEFAULT_WALL_TIME)
}

pub fn run_vm_capture_with_env(
    source_file: &Path,
    source: &str,
    name: &str,
    value: &str,
) -> Result<VmRunOutcome, String> {
    run_vm_capture_with_envs(source_file, source, &[(name, value)], DEFAULT_WALL_TIME)
}

pub fn run_vm_capture_with_wall_time(
    source_file: &Path,
    source: &str,
    wall_time: Duration,
) -> Result<VmRunOutcome, String> {
    run_vm_capture_with_envs(source_file, source, &[], wall_time)
}

fn run_vm_capture_with_envs(
    source_file: &Path,
    source: &str,
    envs: &[(&str, &str)],
    wall_time: Duration,
) -> Result<VmRunOutcome, String> {
    validate_child_envs(envs, VM_RESERVED_ENVS)?;
    let scratch = ScratchDir::new("vm").map_err(|error| error.to_string())?;
    let source_path = scratch.path().join("source.li");
    let result_path = scratch.path().join("result.txt");
    let stdout_path = scratch.path().join("vm-stdout");
    write_source_bounded(&source_path, source)?;

    let mut command = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    command
        .arg("--exact")
        .arg("common::bounded_vm_child")
        .arg("--ignored")
        .arg("--nocapture")
        .arg("--test-threads=1")
        .env(VM_CHILD_ENV, "1")
        .env(VM_SOURCE_ENV, &source_path)
        .env(VM_SOURCE_NAME_ENV, source_file)
        .env(VM_RESULT_ENV, &result_path)
        .env(VM_STDOUT_ENV, &stdout_path);
    for (name, value) in envs {
        command.env(name, value);
    }
    // The bytecode child creates only its small result and capture files.
    apply_output_file_limit(&mut command);

    let output =
        run_command_with_wall_time(&mut command, wall_time).map_err(|error| error.to_string())?;
    output.assert_complete_output()?;
    if output.timed_out {
        return Err(format!(
            "VM child exceeded the {:?} wall-time limit; stderr: {}",
            wall_time,
            output.stderr_text()
        ));
    }
    if output.memory_exceeded {
        return Err(format!(
            "VM child exceeded the {} MiB memory limit; stderr: {}",
            CHILD_MEMORY_LIMIT_BYTES / (1024 * 1024),
            output.stderr_text()
        ));
    }
    if !output.status.success() {
        return Err(format!(
            "VM child failed with {}; stderr: {}",
            output.status,
            output.stderr_text()
        ));
    }

    let metadata = std::fs::read_to_string(&result_path).map_err(|error| {
        format!(
            "VM child did not report a result (status {}, error {error}); stderr: {}",
            output.status,
            output.stderr_text()
        )
    })?;
    let (captured, truncated) = read_bounded_file(&stdout_path)
        .map_err(|error| format!("could not read bounded VM stdout capture: {error}"))?;
    if truncated {
        return Err(format!(
            "VM stdout exceeded the {} byte output limit",
            OUTPUT_LIMIT_BYTES
        ));
    }
    if let Some(status) = metadata.strip_prefix("ok:") {
        let status = status
            .trim()
            .parse::<i32>()
            .map_err(|error| format!("invalid VM child status `{status}`: {error}"))?;
        return Ok(VmRunOutcome::Success {
            status,
            output: captured,
        });
    }
    if let Some(message) = metadata.strip_prefix("compile-error:") {
        return Ok(VmRunOutcome::CompileError(message.trim_end().to_owned()));
    }
    if let Some(message) = metadata.strip_prefix("runtime-error:") {
        return Ok(VmRunOutcome::RuntimeError {
            message: message.trim_end().to_owned(),
            output: captured,
        });
    }
    Err(format!("invalid VM child result: {metadata}"))
}

/// Compile and execute a native program in an isolated, resource-bounded
/// child. The parent test process never hosts generated code or the linker.
pub fn run_aot(source_file: &Path, source: &str) -> Result<BoundedOutput, String> {
    run_aot_with_options(source_file, source, &[], None, DEFAULT_WALL_TIME, false)
}

pub fn run_aot_with_limits(
    source_file: &Path,
    source: &str,
    memory_bytes: u64,
    max_fibers: u64,
    wall_time: Duration,
) -> Result<BoundedOutput, String> {
    run_aot_with_options(
        source_file,
        source,
        &[],
        Some((memory_bytes, max_fibers)),
        wall_time,
        false,
    )
}

pub fn run_aot_with_env(
    source_file: &Path,
    source: &str,
    name: &str,
    value: &str,
) -> Result<BoundedOutput, String> {
    run_aot_with_options(
        source_file,
        source,
        &[(name, value)],
        None,
        DEFAULT_WALL_TIME,
        false,
    )
}

pub fn build_aot(source_file: &Path, source: &str) -> Result<(), String> {
    run_aot_with_options(source_file, source, &[], None, DEFAULT_WALL_TIME, true).map(|_| ())
}

fn run_aot_with_options(
    source_file: &Path,
    source: &str,
    envs: &[(&str, &str)],
    limits: Option<(u64, u64)>,
    wall_time: Duration,
    build_only: bool,
) -> Result<BoundedOutput, String> {
    validate_child_envs(envs, AOT_RESERVED_ENVS)?;
    let scratch = ScratchDir::new("aot").map_err(|error| error.to_string())?;
    let dir = scratch.path();
    // Keep the exact supplied source separate from its logical filename. The
    // latter drives relative import resolution; rereading or creating it in
    // the parent would race callers and could compile stale contents.
    let source_content_path = dir.join("source.li");
    write_source_bounded(&source_content_path, source)?;
    let result_path = dir.join("result.txt");
    let binary_path = dir.join("program");
    let mut command = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    command
        .arg("--exact")
        .arg("common::bounded_aot_child")
        .arg("--ignored")
        .arg("--nocapture")
        .arg("--test-threads=1")
        .env(AOT_CHILD_ENV, "1")
        .env(AOT_SOURCE_ENV, &source_content_path)
        .env(AOT_SOURCE_NAME_ENV, source_file)
        .env(AOT_RESULT_ENV, &result_path)
        .env(AOT_BINARY_ENV, &binary_path)
        .env(AOT_BUILD_ONLY_ENV, if build_only { "1" } else { "0" });
    if let Some((memory_bytes, max_fibers)) = limits {
        command
            .env("LIRA_NATIVE_MEMORY_LIMIT_BYTES", memory_bytes.to_string())
            .env("LIRA_NATIVE_MAX_FIBERS", max_fibers.to_string());
    }
    for (name, value) in envs {
        command.env(name, value);
    }

    let output =
        run_command_with_wall_time(&mut command, wall_time).map_err(|error| error.to_string());
    let result = output.and_then(|output| {
        output.assert_complete_output()?;
        if output.timed_out {
            return Err(format!(
                "native AOT child exceeded the {:?} wall-time limit; stderr: {}",
                wall_time,
                output.stderr_text()
            ));
        }
        if output.memory_exceeded {
            return Err(format!(
                "native AOT child exceeded the {} MiB memory limit; stderr: {}",
                CHILD_MEMORY_LIMIT_BYTES / (1024 * 1024),
                output.stderr_text()
            ));
        }
        if !output.status.success() {
            return Err(format!(
                "native AOT child failed with {}; stderr: {}",
                output.status,
                output.stderr_text()
            ));
        }
        read_aot_result(&result_path)
    });
    result
}

fn scratch_dir(label: &str) -> PathBuf {
    let id = NEXT_SCRATCH_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("lira-bounded-{label}-{}-{id}", std::process::id()))
}

pub fn run_jit(source_file: &str, source: &str) -> Result<i32, String> {
    run_jit_timed(source_file, source).map(|(status, _)| status)
}

pub fn run_jit_timed(source_file: &str, source: &str) -> Result<(i32, Duration), String> {
    run_jit_with_optional_limits(source_file, source, None, DEFAULT_WALL_TIME)
}

/// Run one JIT program in the bounded child and return its exact stdout.
/// The child redirects only the generated program's C/Rust stdout, excluding
/// libtest protocol noise from the captured bytes.
pub fn run_jit_capture(source_file: &str, source: &str) -> Result<(i32, Vec<u8>), String> {
    run_jit_with_optional_limits_and_capture(
        source_file,
        source,
        &[],
        None,
        DEFAULT_WALL_TIME,
        true,
    )
    .map(|(status, _, output)| (status, output))
}

/// Run one bounded JIT child with environment values supplied only to that
/// child process. This keeps hermetic tests from mutating the test process.
pub fn run_jit_capture_with_env(
    source_file: &str,
    source: &str,
    name: &str,
    value: &str,
) -> Result<(i32, Vec<u8>), String> {
    run_jit_with_optional_limits_and_capture(
        source_file,
        source,
        &[(name, value)],
        None,
        DEFAULT_WALL_TIME,
        true,
    )
    .map(|(status, _, output)| (status, output))
}

pub fn run_jit_with_runtime_limits(
    source_file: &str,
    source: &str,
    memory_bytes: u64,
    max_fibers: u64,
) -> Result<i32, String> {
    run_jit_with_optional_limits(
        source_file,
        source,
        Some((memory_bytes, max_fibers)),
        DEFAULT_WALL_TIME,
    )
    .map(|(status, _)| status)
}

pub fn run_jit_with_limits(
    source_file: &str,
    source: &str,
    memory_bytes: u64,
    max_fibers: u64,
    wall_time: Duration,
) -> Result<i32, String> {
    run_jit_with_optional_limits(
        source_file,
        source,
        Some((memory_bytes, max_fibers)),
        wall_time,
    )
    .map(|(status, _)| status)
}

/// Run several programs through one JIT runtime instance inside a bounded
/// child. Tests for cleanup and recovery need this same-process sequence, but
/// the parent test process must not host the generated code itself.
pub fn run_jit_sequence(programs: &[(&str, &str)]) -> Result<Vec<Result<i32, String>>, String> {
    run_jit_sequence_timed(programs).map(|(results, _)| results)
}

pub fn run_jit_sequence_timed(
    programs: &[(&str, &str)],
) -> Result<(Vec<Result<i32, String>>, Duration), String> {
    if programs.is_empty() {
        return Ok((Vec::new(), Duration::ZERO));
    }
    validate_jit_sequence_inputs(programs)?;
    let scratch = ScratchDir::new("jit-sequence").map_err(|error| error.to_string())?;
    let dir = scratch.path();
    for (index, (source_name, source)) in programs.iter().enumerate() {
        write_source_bounded(&dir.join(format!("source-{index}.li")), source)?;
        std::fs::write(dir.join(format!("source-{index}.name")), source_name)
            .map_err(|error| error.to_string())?;
    }

    let mut command = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    command
        .arg("--exact")
        .arg("common::bounded_jit_child")
        .arg("--ignored")
        .arg("--nocapture")
        .arg("--test-threads=1")
        .env(JIT_CHILD_ENV, "1")
        .env(JIT_SEQUENCE_DIR_ENV, dir)
        .env(JIT_SEQUENCE_COUNT_ENV, programs.len().to_string());
    // A JIT child only reads its prepared sources and writes small result
    // records. Unlike the AOT compiler wrapper, it never needs to create
    // object files, archives, or an executable, so its inherited output
    // descriptors can have an OS-enforced hard ceiling.
    apply_output_file_limit(&mut command);

    let output = run_command(&mut command).map_err(|error| error.to_string())?;
    output.assert_complete_output()?;
    let results = if output.timed_out {
        Err(format!(
            "JIT sequence exceeded the {:?} wall-time limit; stderr: {}",
            DEFAULT_WALL_TIME,
            output.stderr_text()
        ))
    } else if output.memory_exceeded {
        Err(format!(
            "JIT sequence exceeded the {} MiB memory limit; stderr: {}",
            CHILD_MEMORY_LIMIT_BYTES / (1024 * 1024),
            output.stderr_text()
        ))
    } else if output.stdout_truncated || output.stderr_truncated {
        Err(format!(
            "JIT sequence exceeded the output limit; stderr: {}",
            output.stderr_text()
        ))
    } else if !output.status.success() {
        Err(format!(
            "JIT sequence child failed with {}; stderr: {}",
            output.status,
            output.stderr_text()
        ))
    } else {
        (0..programs.len())
            .map(|index| {
                let path = dir.join(format!("result-{index}.txt"));
                let result = std::fs::read_to_string(&path).map_err(|error| {
                    format!(
                        "JIT sequence child did not report result {index} (status {}, error {error}); stderr: {}",
                        output.status,
                        output.stderr_text()
                    )
                })?;
                Ok(parse_jit_result(&result))
            })
            .collect::<Result<Vec<_>, String>>()
    };
    results.map(|results| (results, output.elapsed))
}

fn validate_jit_sequence_inputs(programs: &[(&str, &str)]) -> Result<(), String> {
    if programs.len() > MAX_JIT_PROGRAMS {
        return Err(format!(
            "JIT sequence contains {} programs, exceeding the {} program limit",
            programs.len(),
            MAX_JIT_PROGRAMS
        ));
    }
    let mut total_bytes = 0usize;
    for (index, (_, source)) in programs.iter().enumerate() {
        if source.len() > SOURCE_LIMIT_BYTES {
            return Err(format!(
                "JIT sequence source {index} exceeds the {} byte limit",
                SOURCE_LIMIT_BYTES
            ));
        }
        total_bytes = total_bytes
            .checked_add(source.len())
            .ok_or_else(|| "JIT sequence aggregate source size overflowed".to_owned())?;
        if total_bytes > MAX_JIT_SEQUENCE_SOURCE_BYTES {
            return Err(format!(
                "JIT sequence aggregate source exceeds the {} byte limit",
                MAX_JIT_SEQUENCE_SOURCE_BYTES
            ));
        }
    }
    Ok(())
}

fn run_jit_with_optional_limits(
    source_file: &str,
    source: &str,
    limits: Option<(u64, u64)>,
    wall_time: Duration,
) -> Result<(i32, Duration), String> {
    run_jit_with_optional_limits_and_capture(source_file, source, &[], limits, wall_time, false)
        .map(|(status, elapsed, _)| (status, elapsed))
}

fn run_jit_with_optional_limits_and_capture(
    source_file: &str,
    source: &str,
    envs: &[(&str, &str)],
    limits: Option<(u64, u64)>,
    wall_time: Duration,
    capture_stdout: bool,
) -> Result<(i32, Duration, Vec<u8>), String> {
    validate_child_envs(envs, JIT_RESERVED_ENVS)?;
    let scratch = ScratchDir::new("jit").map_err(|error| error.to_string())?;
    let dir = scratch.path();
    let source_path = dir.join(
        Path::new(source_file)
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("program.li")),
    );
    let result_path = dir.join("result.txt");
    let stdout_path = dir.join("jit-stdout");
    write_source_bounded(&source_path, source)?;

    let mut command = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    command
        .arg("--exact")
        .arg("common::bounded_jit_child")
        .arg("--ignored")
        .arg("--nocapture")
        .arg("--test-threads=1")
        .env(JIT_CHILD_ENV, "1")
        .env(JIT_SOURCE_ENV, &source_path)
        .env(JIT_SOURCE_NAME_ENV, source_file)
        .env(JIT_RESULT_ENV, &result_path);
    // This limit protects both the outer supervisor's redirected stdout and
    // the optional private capture file. JIT does not emit AOT artifacts, so
    // it is safe to enforce it for every JIT invocation.
    apply_output_file_limit(&mut command);
    if capture_stdout {
        command.env(JIT_STDOUT_ENV, &stdout_path);
    }
    if let Some((memory_bytes, max_fibers)) = limits {
        command
            .env("LIRA_NATIVE_MEMORY_LIMIT_BYTES", memory_bytes.to_string())
            .env("LIRA_NATIVE_MAX_FIBERS", max_fibers.to_string());
    }
    for (name, value) in envs {
        command.env(name, value);
    }

    let output =
        run_command_with_wall_time(&mut command, wall_time).map_err(|error| error.to_string())?;
    output.assert_complete_output()?;
    let result = if output.timed_out {
        Err(format!(
            "JIT child exceeded the {:?} wall-time limit; stderr: {}",
            wall_time,
            output.stderr_text()
        ))
    } else if output.memory_exceeded {
        Err(format!(
            "JIT child exceeded the {} MiB memory limit; stderr: {}",
            CHILD_MEMORY_LIMIT_BYTES / (1024 * 1024),
            output.stderr_text()
        ))
    } else if output.stdout_truncated || output.stderr_truncated {
        Err(format!(
            "JIT child exceeded the output limit; stderr: {}",
            output.stderr_text()
        ))
    } else if !output.status.success() {
        Err(format!(
            "JIT child failed with {}; stderr: {}",
            output.status,
            output.stderr_text()
        ))
    } else {
        let result = std::fs::read_to_string(&result_path).map_err(|error| {
            format!(
                "JIT child did not report a result (status {}, error {error}); stderr: {}",
                output.status,
                output.stderr_text()
            )
        })?;
        parse_jit_result(&result)
    };
    let status = result?;
    let captured = if capture_stdout {
        let (captured, truncated) = read_bounded_file(&stdout_path)
            .map_err(|error| format!("could not read bounded JIT stdout capture: {error}"))?;
        if truncated {
            return Err(format!(
                "JIT stdout exceeded the {} byte output limit",
                OUTPUT_LIMIT_BYTES
            ));
        }
        captured
    } else {
        Vec::new()
    };
    Ok((status, output.elapsed, captured))
}

fn parse_jit_result(result: &str) -> Result<i32, String> {
    match result.strip_prefix("ok:") {
        Some(status) => status
            .trim()
            .parse::<i32>()
            .map_err(|error| format!("invalid JIT child status `{status}`: {error}")),
        None => Err(result
            .strip_prefix("err:")
            .unwrap_or(result)
            .trim()
            .to_owned()),
    }
}

fn read_bounded_file_with_limit(path: &Path, limit: usize) -> io::Result<(Vec<u8>, bool)> {
    let mut file = File::open(path)?;
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8192];
    let mut truncated = false;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let remaining = limit.saturating_sub(output.len());
        let take = remaining.min(count);
        output.extend_from_slice(&buffer[..take]);
        truncated |= take != count;
        if truncated {
            break;
        }
    }
    Ok((output, truncated))
}

fn read_bounded_file(path: &Path) -> io::Result<(Vec<u8>, bool)> {
    read_bounded_file_with_limit(path, OUTPUT_LIMIT_BYTES)
}

pub fn read_source_bounded(path: &Path) -> Result<String, String> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| format!("could not open source {}: {error}", path.display()))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("could not inspect source {}: {error}", path.display()))?;
    if !metadata.file_type().is_file() {
        return Err(format!("source {} is not a regular file", path.display()));
    }
    if metadata.len() > SOURCE_LIMIT_BYTES as u64 {
        return Err(format!(
            "source {} exceeds the {} byte limit",
            path.display(),
            SOURCE_LIMIT_BYTES
        ));
    }
    let mut source = Vec::with_capacity(metadata.len() as usize);
    let mut limited = (&mut file).take((SOURCE_LIMIT_BYTES + 1) as u64);
    limited
        .read_to_end(&mut source)
        .map_err(|error| format!("could not read source {}: {error}", path.display()))?;
    let truncated = source.len() > SOURCE_LIMIT_BYTES;
    if truncated {
        return Err(format!(
            "source {} exceeds the {} byte limit",
            path.display(),
            SOURCE_LIMIT_BYTES
        ));
    }
    String::from_utf8(source)
        .map_err(|error| format!("source {} is not valid UTF-8: {error}", path.display()))
}

fn write_source_bounded(path: &Path, source: &str) -> Result<(), String> {
    if source.len() > SOURCE_LIMIT_BYTES {
        return Err(format!(
            "source exceeds the {} byte limit",
            SOURCE_LIMIT_BYTES
        ));
    }
    std::fs::write(path, source).map_err(|error| error.to_string())
}

const FRONTEND_FRAME_MAGIC: &[u8; 4] = b"LFP1";

fn encode_frontend_frame(outcome: &FrontendPreflightOutcome) -> Result<Vec<u8>, String> {
    let diagnostics = match outcome {
        FrontendPreflightOutcome::Accepted => &[] as &[FrontendDiagnostic],
        FrontendPreflightOutcome::CompileError(diagnostics) => diagnostics.as_slice(),
    };
    if diagnostics.len() > FRONTEND_DIAGNOSTIC_LIMIT {
        return Err(format!(
            "frontend returned {} diagnostics, exceeding the {} diagnostic limit",
            diagnostics.len(),
            FRONTEND_DIAGNOSTIC_LIMIT
        ));
    }

    let mut payload = Vec::new();
    payload.push(u8::from(!matches!(
        outcome,
        FrontendPreflightOutcome::Accepted
    )));
    let count = u32::try_from(diagnostics.len())
        .map_err(|_| "frontend diagnostic count does not fit the frame".to_owned())?;
    payload.extend_from_slice(&count.to_le_bytes());
    for diagnostic in diagnostics {
        let line = u64::try_from(diagnostic.line)
            .map_err(|_| "frontend diagnostic line does not fit the frame".to_owned())?;
        let column = u64::try_from(diagnostic.column)
            .map_err(|_| "frontend diagnostic column does not fit the frame".to_owned())?;
        if diagnostic.message.len() > FRONTEND_MESSAGE_LIMIT_BYTES {
            return Err(format!(
                "frontend diagnostic message exceeds the {} byte limit",
                FRONTEND_MESSAGE_LIMIT_BYTES
            ));
        }
        let message_len = u32::try_from(diagnostic.message.len())
            .map_err(|_| "frontend diagnostic message does not fit the frame".to_owned())?;
        payload.extend_from_slice(&line.to_le_bytes());
        payload.extend_from_slice(&column.to_le_bytes());
        payload.extend_from_slice(&message_len.to_le_bytes());
        payload.extend_from_slice(diagnostic.message.as_bytes());
    }
    let payload_len = u32::try_from(payload.len())
        .map_err(|_| "frontend result frame is too large".to_owned())?;
    let frame_len = FRONTEND_FRAME_MAGIC
        .len()
        .saturating_add(std::mem::size_of::<u32>())
        .saturating_add(payload.len());
    if frame_len > FRONTEND_FRAME_LIMIT_BYTES {
        return Err(format!(
            "frontend result frame exceeds the {} byte limit",
            FRONTEND_FRAME_LIMIT_BYTES
        ));
    }
    let mut frame = Vec::with_capacity(frame_len);
    frame.extend_from_slice(FRONTEND_FRAME_MAGIC);
    frame.extend_from_slice(&payload_len.to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

fn read_u32_frame(bytes: &[u8], offset: &mut usize) -> Result<u32, String> {
    let end = offset
        .checked_add(std::mem::size_of::<u32>())
        .ok_or_else(|| "frontend result frame offset overflowed".to_owned())?;
    let value = bytes
        .get(*offset..end)
        .ok_or_else(|| "frontend result frame is truncated".to_owned())?;
    *offset = end;
    Ok(u32::from_le_bytes(value.try_into().map_err(|_| {
        "frontend result frame has an invalid u32".to_owned()
    })?))
}

fn read_u64_frame(bytes: &[u8], offset: &mut usize) -> Result<u64, String> {
    let end = offset
        .checked_add(std::mem::size_of::<u64>())
        .ok_or_else(|| "frontend result frame offset overflowed".to_owned())?;
    let value = bytes
        .get(*offset..end)
        .ok_or_else(|| "frontend result frame is truncated".to_owned())?;
    *offset = end;
    Ok(u64::from_le_bytes(value.try_into().map_err(|_| {
        "frontend result frame has an invalid u64".to_owned()
    })?))
}

fn decode_frontend_frame(bytes: &[u8]) -> Result<FrontendPreflightOutcome, String> {
    if bytes.len() > FRONTEND_FRAME_LIMIT_BYTES {
        return Err("frontend result frame exceeds its byte limit".to_owned());
    }
    let header_len = FRONTEND_FRAME_MAGIC.len() + std::mem::size_of::<u32>();
    if bytes.len() < header_len || &bytes[..FRONTEND_FRAME_MAGIC.len()] != FRONTEND_FRAME_MAGIC {
        return Err("frontend result frame has an invalid header".to_owned());
    }
    let mut offset = FRONTEND_FRAME_MAGIC.len();
    let payload_len = usize::try_from(read_u32_frame(bytes, &mut offset)?)
        .map_err(|_| "frontend result frame length does not fit this platform".to_owned())?;
    let frame_len = header_len
        .checked_add(payload_len)
        .ok_or_else(|| "frontend result frame length overflowed".to_owned())?;
    if frame_len != bytes.len() {
        return Err("frontend result frame is truncated or has trailing bytes".to_owned());
    }
    let kind = *bytes
        .get(offset)
        .ok_or_else(|| "frontend result frame is missing its result kind".to_owned())?;
    offset += 1;
    let count = usize::try_from(read_u32_frame(bytes, &mut offset)?)
        .map_err(|_| "frontend diagnostic count does not fit this platform".to_owned())?;
    if count > FRONTEND_DIAGNOSTIC_LIMIT {
        return Err(format!(
            "frontend result frame has {} diagnostics, exceeding the {} diagnostic limit",
            count, FRONTEND_DIAGNOSTIC_LIMIT
        ));
    }
    let mut diagnostics = Vec::with_capacity(count);
    for _ in 0..count {
        let line = usize::try_from(read_u64_frame(bytes, &mut offset)?)
            .map_err(|_| "frontend diagnostic line does not fit this platform".to_owned())?;
        let column = usize::try_from(read_u64_frame(bytes, &mut offset)?)
            .map_err(|_| "frontend diagnostic column does not fit this platform".to_owned())?;
        let message_len = usize::try_from(read_u32_frame(bytes, &mut offset)?).map_err(|_| {
            "frontend diagnostic message length does not fit this platform".to_owned()
        })?;
        if message_len > FRONTEND_MESSAGE_LIMIT_BYTES {
            return Err(format!(
                "frontend diagnostic message exceeds the {} byte limit",
                FRONTEND_MESSAGE_LIMIT_BYTES
            ));
        }
        let end = offset
            .checked_add(message_len)
            .ok_or_else(|| "frontend diagnostic message offset overflowed".to_owned())?;
        let message = bytes
            .get(offset..end)
            .ok_or_else(|| "frontend result frame is truncated".to_owned())?;
        let message = String::from_utf8(message.to_vec())
            .map_err(|_| "frontend diagnostic message is not valid UTF-8".to_owned())?;
        diagnostics.push(FrontendDiagnostic {
            line,
            column,
            message,
        });
        offset = end;
    }
    if offset != bytes.len() {
        return Err("frontend result frame has trailing payload bytes".to_owned());
    }
    match kind {
        0 if diagnostics.is_empty() => Ok(FrontendPreflightOutcome::Accepted),
        1 if !diagnostics.is_empty() => Ok(FrontendPreflightOutcome::CompileError(diagnostics)),
        _ => Err("frontend result frame has an invalid result kind".to_owned()),
    }
}

/// Run only the compiler front end in an isolated, resource-bounded child.
/// `Err` is reserved for containment, infrastructure, or protocol failures;
/// source diagnostics are returned as [`FrontendPreflightOutcome::CompileError`].
pub fn run_frontend_preflight(
    source_file: &Path,
    source: &str,
) -> Result<FrontendPreflightOutcome, String> {
    if source.len() > SOURCE_LIMIT_BYTES {
        return Err(format!(
            "source exceeds the {} byte limit",
            SOURCE_LIMIT_BYTES
        ));
    }
    let source_name = source_file
        .to_str()
        .ok_or_else(|| "frontend logical source path is not valid UTF-8".to_owned())?;
    let scratch = ScratchDir::new("frontend").map_err(|error| error.to_string())?;
    let source_path = scratch.path().join("source.li");
    let result_path = scratch.path().join("result.bin");
    write_source_bounded(&source_path, source)?;

    let mut command = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    command
        .arg("--exact")
        .arg("common::bounded_frontend_child")
        .arg("--ignored")
        .arg("--nocapture")
        .arg("--test-threads=1")
        .env(FRONTEND_CHILD_ENV, "1")
        .env(FRONTEND_SOURCE_ENV, &source_path)
        .env(FRONTEND_SOURCE_NAME_ENV, source_name)
        .env(FRONTEND_RESULT_ENV, &result_path);
    apply_output_file_limit(&mut command);
    let output = run_command(&mut command).map_err(|error| error.to_string())?;
    if output.timed_out {
        return Err(format!(
            "frontend child exceeded the {:?} wall-time limit; stderr: {}",
            DEFAULT_WALL_TIME,
            output.stderr_text()
        ));
    }
    if output.memory_exceeded {
        return Err(format!(
            "frontend child exceeded the {} MiB memory limit; stderr: {}",
            CHILD_MEMORY_LIMIT_BYTES / (1024 * 1024),
            output.stderr_text()
        ));
    }
    output.assert_complete_output()?;
    if !output.status.success() {
        return Err(format!(
            "frontend child failed with {}; stderr: {}",
            output.status,
            output.stderr_text()
        ));
    }
    let (frame, truncated) = read_bounded_file_with_limit(&result_path, FRONTEND_FRAME_LIMIT_BYTES)
        .map_err(|error| format!("could not read frontend result frame: {error}"))?;
    if truncated {
        return Err(format!(
            "frontend result frame exceeded the {} byte limit",
            FRONTEND_FRAME_LIMIT_BYTES
        ));
    }
    decode_frontend_frame(&frame)
}

fn write_aot_result(path: &Path, output: &BoundedOutput) -> io::Result<()> {
    let raw_status = output.status.into_raw();
    let seconds = output.elapsed.as_secs();
    let nanos = output.elapsed.subsec_nanos();
    let metadata = format!(
        "ok\n{raw_status}\n{}\n{}\n{}\n{}\n{seconds}\n{nanos}\n",
        output.timed_out, output.memory_exceeded, output.stdout_truncated, output.stderr_truncated,
    );
    std::fs::write(path, metadata)
}

fn write_aot_error(path: &Path, error: &str) -> io::Result<()> {
    let mut result = String::from("error\n");
    result.push_str(error);
    std::fs::write(path, result)
}

fn read_aot_result(path: &Path) -> Result<BoundedOutput, String> {
    let metadata = std::fs::read_to_string(path).map_err(|error| {
        format!(
            "native AOT child did not report a result ({}): {error}",
            path.display()
        )
    })?;
    if let Some(error) = metadata.strip_prefix("error\n") {
        return Err(error.trim_end().to_owned());
    }
    let mut lines = metadata.lines();
    if lines.next() != Some("ok") {
        return Err(format!("invalid native AOT child result: {metadata}"));
    }
    let raw_status = lines
        .next()
        .ok_or_else(|| "native AOT child result is missing its status".to_owned())?
        .parse::<i32>()
        .map_err(|error| format!("invalid native AOT child status: {error}"))?;
    let timed_out = parse_bool_result_line(&mut lines, "timed-out")?;
    let memory_exceeded = parse_bool_result_line(&mut lines, "memory-exceeded")?;
    let stdout_truncated = parse_bool_result_line(&mut lines, "stdout-truncated")?;
    let stderr_truncated = parse_bool_result_line(&mut lines, "stderr-truncated")?;
    let seconds = lines
        .next()
        .ok_or_else(|| "native AOT child result is missing elapsed seconds".to_owned())?
        .parse::<u64>()
        .map_err(|error| format!("invalid native AOT elapsed seconds: {error}"))?;
    let nanos = lines
        .next()
        .ok_or_else(|| "native AOT child result is missing elapsed nanoseconds".to_owned())?
        .parse::<u32>()
        .map_err(|error| format!("invalid native AOT elapsed nanoseconds: {error}"))?;
    let stdout_path = path.with_file_name("stdout");
    let stderr_path = path.with_file_name("stderr");
    let (stdout, stdout_file_truncated) =
        read_bounded_file(&stdout_path).map_err(|error| error.to_string())?;
    let (stderr, stderr_file_truncated) =
        read_bounded_file(&stderr_path).map_err(|error| error.to_string())?;
    Ok(BoundedOutput {
        status: ExitStatus::from_raw(raw_status),
        stdout,
        stderr,
        elapsed: Duration::new(seconds, nanos),
        timed_out,
        memory_exceeded,
        stdout_truncated: stdout_truncated || stdout_file_truncated,
        stderr_truncated: stderr_truncated || stderr_file_truncated,
    })
}

fn parse_bool_result_line<'a>(
    lines: &mut impl Iterator<Item = &'a str>,
    field: &str,
) -> Result<bool, String> {
    let value = lines
        .next()
        .ok_or_else(|| format!("native AOT child result is missing {field}"))?;
    value
        .parse::<bool>()
        .map_err(|error| format!("invalid native AOT {field}: {error}"))
}

fn run_aot_binary_unlocked(
    binary_path: &Path,
    stdout_path: &Path,
    stderr_path: &Path,
) -> io::Result<BoundedOutput> {
    let stdout = File::create(stdout_path)?;
    let stderr = File::create(stderr_path)?;
    let mut command = Command::new(binary_path);
    command
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    // This executable is a descendant of the already bounded AOT wrapper. It
    // must stay in that wrapper's process group so the outer supervisor measures
    // and kills the whole compile -> link -> execute tree. CPU, memory-policy,
    // and address-space limits are inherited across exec.
    apply_output_file_limit(&mut command);
    let mut child = command.spawn()?;
    let started = Instant::now();
    let (status, mut stdout_truncated, mut stderr_truncated) = loop {
        if let Some(status) = child.try_wait()? {
            break (status, false, false);
        }
        let (stdout_len, stderr_len) = match output_file_sizes(stdout_path, stderr_path) {
            Ok(sizes) => sizes,
            Err(error) => {
                let kill_error = kill_child_only(&mut child).err();
                return Err(match kill_error {
                    Some(kill_error) => io::Error::other(format!(
                        "could not inspect generated AOT output files: {error}; \
                         could not kill executable: {kill_error}"
                    )),
                    None => io::Error::other(format!(
                        "could not inspect generated AOT output files: {error}"
                    )),
                });
            }
        };
        let (stdout_truncated, stderr_truncated) = output_limit_flags(stdout_len, stderr_len);
        if stdout_truncated || stderr_truncated {
            break (
                kill_child_only(&mut child)?,
                stdout_truncated,
                stderr_truncated,
            );
        }
        thread::sleep(Duration::from_millis(10));
    };
    // The generated executable deliberately remains in the outer AOT
    // wrapper's group. The outer supervisor kills and verifies that group once
    // this wrapper exits; using the executable PID as a group ID here would be
    // incorrect and could race PID reuse after it has been reaped.
    let (final_stdout_len, final_stderr_len) = output_file_sizes(stdout_path, stderr_path)?;
    let (final_stdout_truncated, final_stderr_truncated) =
        output_limit_flags(final_stdout_len, final_stderr_len);
    stdout_truncated |= final_stdout_truncated;
    stderr_truncated |= final_stderr_truncated;
    let (stdout, stdout_file_truncated) = read_bounded_file(stdout_path)?;
    let (stderr, stderr_file_truncated) = read_bounded_file(stderr_path)?;
    Ok(BoundedOutput {
        status,
        stdout,
        stderr,
        elapsed: started.elapsed(),
        timed_out: false,
        memory_exceeded: false,
        stdout_truncated: stdout_truncated || stdout_file_truncated,
        stderr_truncated: stderr_truncated || stderr_file_truncated,
    })
}

#[test]
#[ignore = "invoked only by run_vm_capture in an isolated child process"]
fn bounded_vm_child() {
    if std::env::var_os(VM_CHILD_ENV).is_none() {
        return;
    }
    let source_path =
        PathBuf::from(std::env::var_os(VM_SOURCE_ENV).expect("bounded VM child source path"));
    let source_name = PathBuf::from(
        std::env::var_os(VM_SOURCE_NAME_ENV).expect("bounded VM child logical source name"),
    );
    let result_path =
        PathBuf::from(std::env::var_os(VM_RESULT_ENV).expect("bounded VM child result path"));
    let stdout_path =
        PathBuf::from(std::env::var_os(VM_STDOUT_ENV).expect("bounded VM child stdout path"));
    let source = std::fs::read_to_string(&source_path).expect("read bounded VM child source");

    let (metadata, lines) = match lirac::compile_with_imports(
        source_name
            .to_str()
            .expect("UTF-8 bounded VM logical source path"),
        &source,
    ) {
        Err(error) => (format!("compile-error:{error}"), Vec::new()),
        Ok(bytecode) => match liravm::run_with_capture_structured(&bytecode) {
            Ok((0, lines)) => ("ok:0\n".to_owned(), lines),
            Ok((status, lines)) => (
                format!("runtime-error:VM exited with status {status}"),
                lines,
            ),
            Err((lines, error)) => (format!("runtime-error:{}", error.message), lines),
        },
    };
    std::fs::write(&stdout_path, lines.join("\n")).expect("write bounded VM stdout");
    std::fs::write(&result_path, metadata).expect("write bounded VM result");
}

#[test]
#[ignore = "invoked only by run_frontend_preflight in an isolated child process"]
fn bounded_frontend_child() {
    if std::env::var_os(FRONTEND_CHILD_ENV).is_none() {
        return;
    }
    let Some(source_path) = std::env::var_os(FRONTEND_SOURCE_ENV).map(PathBuf::from) else {
        return;
    };
    let Some(source_name) = std::env::var_os(FRONTEND_SOURCE_NAME_ENV) else {
        return;
    };
    let Some(result_path) = std::env::var_os(FRONTEND_RESULT_ENV).map(PathBuf::from) else {
        return;
    };
    let Ok(source_name) = source_name.into_string() else {
        return;
    };
    let Ok(source) = read_source_bounded(&source_path) else {
        return;
    };
    let outcome = match lirac::analyze_with_imports(&source_name, &source) {
        Ok(analysis) if analysis.diagnostics.is_empty() => FrontendPreflightOutcome::Accepted,
        Ok(analysis) => FrontendPreflightOutcome::CompileError(
            analysis
                .diagnostics
                .into_iter()
                .map(|diagnostic| FrontendDiagnostic {
                    line: diagnostic.line,
                    column: diagnostic.column,
                    message: diagnostic.message,
                })
                .collect(),
        ),
        Err(diagnostic) => FrontendPreflightOutcome::CompileError(vec![FrontendDiagnostic {
            line: diagnostic.line,
            column: diagnostic.column,
            message: diagnostic.message,
        }]),
    };
    let Ok(frame) = encode_frontend_frame(&outcome) else {
        return;
    };
    let _ = std::fs::write(result_path, frame);
}

#[test]
fn frontend_protocol_round_trips_utf8_and_newlines() {
    let outcome = FrontendPreflightOutcome::CompileError(vec![FrontendDiagnostic {
        line: 7,
        column: 11,
        message: "first line\nsecond line — UTF-8".to_owned(),
    }]);
    let frame = encode_frontend_frame(&outcome).expect("encode frontend frame");
    assert_eq!(decode_frontend_frame(&frame), Ok(outcome));
}

#[test]
fn frontend_protocol_rejects_truncated_and_trailing_frames() {
    let frame = encode_frontend_frame(&FrontendPreflightOutcome::Accepted)
        .expect("encode accepted frontend frame");
    assert!(decode_frontend_frame(&frame[..frame.len() - 1]).is_err());
    let mut trailing = frame;
    trailing.push(0);
    assert!(decode_frontend_frame(&trailing).is_err());
}

#[test]
fn source_limit_is_checked_before_any_child_launch() {
    let source = "x".repeat(SOURCE_LIMIT_BYTES + 1);
    let path = Path::new("logical.li");
    assert!(run_frontend_preflight(path, &source).is_err());
    assert!(run_vm_capture(path, &source).is_err());
    assert!(run_aot(path, &source).is_err());
    assert!(run_jit_capture("logical.li", &source).is_err());
    assert!(run_jit_sequence(&[("logical.li", &source)]).is_err());
}

#[test]
fn jit_sequence_validates_count_and_all_sources_before_setup() {
    let oversized = "x".repeat(SOURCE_LIMIT_BYTES + 1);
    assert!(
        validate_jit_sequence_inputs(&[("first.li", "ok"), ("second.li", &oversized),]).is_err()
    );

    let programs = (0..=MAX_JIT_PROGRAMS)
        .map(|_| ("program.li", "ok"))
        .collect::<Vec<_>>();
    assert!(validate_jit_sequence_inputs(&programs).is_err());

    let half = "x".repeat(MAX_JIT_SEQUENCE_SOURCE_BYTES / 2 + 1);
    assert!(validate_jit_sequence_inputs(&[("a.li", &half), ("b.li", &half)]).is_err());
}

#[test]
fn caller_environment_cannot_replace_bounded_child_protocol() {
    assert!(
        validate_child_envs(&[(VM_SOURCE_ENV, "attacker-controlled")], VM_RESERVED_ENVS,).is_err()
    );
    assert!(validate_child_envs(
        &[(AOT_RESULT_ENV, "attacker-controlled")],
        AOT_RESERVED_ENVS,
    )
    .is_err());
    assert!(validate_child_envs(
        &[(JIT_SEQUENCE_DIR_ENV, "attacker-controlled")],
        JIT_RESERVED_ENVS,
    )
    .is_err());
    assert!(validate_child_envs(
        &[("LIRA_CRAWLER_BASE_URL", "http://127.0.0.1")],
        JIT_RESERVED_ENVS,
    )
    .is_ok());
}

#[test]
fn bounded_output_rejects_invalid_utf8() {
    let output = BoundedOutput {
        status: ExitStatus::from_raw(0),
        stdout: vec![0xff],
        stderr: Vec::new(),
        elapsed: Duration::ZERO,
        timed_out: false,
        memory_exceeded: false,
        stdout_truncated: false,
        stderr_truncated: false,
    };
    assert!(output.assert_complete_output().is_err());
}

#[test]
#[ignore = "invoked only by run_aot in an isolated child process"]
fn bounded_aot_child() {
    if std::env::var_os(AOT_CHILD_ENV).is_none() {
        return;
    }
    let source_path = PathBuf::from(std::env::var_os(AOT_SOURCE_ENV).expect("bounded AOT source"));
    let source_name = PathBuf::from(
        std::env::var_os(AOT_SOURCE_NAME_ENV).expect("bounded AOT logical source name"),
    );
    let result_path = PathBuf::from(std::env::var_os(AOT_RESULT_ENV).expect("bounded AOT result"));
    let binary_path = PathBuf::from(std::env::var_os(AOT_BINARY_ENV).expect("bounded AOT binary"));
    let source = match std::fs::read_to_string(&source_path) {
        Ok(source) => source,
        Err(error) => {
            write_aot_error(&result_path, &format!("could not read AOT source: {error}"))
                .expect("write bounded AOT error");
            return;
        }
    };
    if let Err(error) = lira_codegen::build_native(
        source_name
            .to_str()
            .expect("UTF-8 bounded AOT logical source path"),
        &source,
        &binary_path,
    ) {
        write_aot_error(&result_path, &error).expect("write bounded AOT build error");
        return;
    }
    if std::env::var_os(AOT_BUILD_ONLY_ENV).as_deref() == Some(std::ffi::OsStr::new("1")) {
        File::create(result_path.with_file_name("stdout")).expect("create bounded AOT stdout");
        File::create(result_path.with_file_name("stderr")).expect("create bounded AOT stderr");
        let output = BoundedOutput {
            status: ExitStatus::from_raw(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
            elapsed: Duration::ZERO,
            timed_out: false,
            memory_exceeded: false,
            stdout_truncated: false,
            stderr_truncated: false,
        };
        write_aot_result(&result_path, &output).expect("write bounded AOT build result");
        return;
    }
    let output = run_aot_binary_unlocked(
        &binary_path,
        &result_path.with_file_name("stdout"),
        &result_path.with_file_name("stderr"),
    )
    .expect("run bounded AOT binary");
    write_aot_result(&result_path, &output).expect("write bounded AOT result");
}

struct StdoutRedirect {
    saved_fd: libc::c_int,
}

impl StdoutRedirect {
    fn to_file(path: &Path) -> io::Result<Self> {
        std::io::stdout().flush()?;
        // SAFETY: flushing all C streams before replacing stdout prevents
        // buffered libtest output from being written into the private file.
        unsafe {
            libc::fflush(std::ptr::null_mut());
        }
        let output = File::create(path)?;
        // SAFETY: STDOUT_FILENO and `output` are valid descriptors here. The
        // saved duplicate remains owned by the guard until restoration.
        let saved_fd = unsafe { libc::dup(libc::STDOUT_FILENO) };
        if saved_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { libc::dup2(output.as_raw_fd(), libc::STDOUT_FILENO) } < 0 {
            let error = io::Error::last_os_error();
            unsafe {
                libc::close(saved_fd);
            }
            return Err(error);
        }
        Ok(Self { saved_fd })
    }
}

impl Drop for StdoutRedirect {
    fn drop(&mut self) {
        let _ = std::io::stdout().flush();
        // SAFETY: the guard exclusively owns `saved_fd`; restore it over the
        // process stdout descriptor, then release the duplicate.
        unsafe {
            libc::fflush(std::ptr::null_mut());
            let _ = libc::dup2(self.saved_fd, libc::STDOUT_FILENO);
            let _ = libc::close(self.saved_fd);
        }
    }
}

fn execute_jit_child(source_path: &Path, source_name: &str, result_path: &Path) {
    let source = std::fs::read_to_string(source_path).expect("read bounded JIT child source");
    let redirect = std::env::var_os(JIT_STDOUT_ENV)
        .map(PathBuf::from)
        .map(|path| StdoutRedirect::to_file(&path))
        .transpose();
    let result = match redirect {
        Ok(redirect) => {
            let result = lira_codegen::jit_run_in_process(source_name, &source);
            drop(redirect);
            result
        }
        Err(error) => Err(format!("could not capture bounded JIT stdout: {error}")),
    };
    let result = match result {
        Ok(status) => format!("ok:{status}\n"),
        Err(error) => format!("err:{error}\n"),
    };
    std::fs::write(result_path, result).expect("write bounded JIT child result");
}

#[test]
#[ignore = "invoked only by run_jit in an isolated child process"]
fn bounded_jit_child() {
    if std::env::var_os(JIT_CHILD_ENV).is_none() {
        return;
    }
    if let Some(dir) = std::env::var_os(JIT_SEQUENCE_DIR_ENV) {
        let dir = PathBuf::from(dir);
        let count = std::env::var(JIT_SEQUENCE_COUNT_ENV)
            .expect("bounded JIT sequence count")
            .parse::<usize>()
            .expect("valid bounded JIT sequence count");
        for index in 0..count {
            let source_name = std::fs::read_to_string(dir.join(format!("source-{index}.name")))
                .expect("read bounded JIT source name");
            execute_jit_child(
                &dir.join(format!("source-{index}.li")),
                &source_name,
                &dir.join(format!("result-{index}.txt")),
            );
        }
        return;
    }
    let source_path =
        PathBuf::from(std::env::var_os(JIT_SOURCE_ENV).expect("bounded JIT child source path"));
    let source_name =
        std::env::var(JIT_SOURCE_NAME_ENV).expect("bounded JIT child logical source name");
    let result_path =
        PathBuf::from(std::env::var_os(JIT_RESULT_ENV).expect("bounded JIT child result path"));
    execute_jit_child(&source_path, &source_name, &result_path);
}
