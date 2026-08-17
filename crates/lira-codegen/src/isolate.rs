//! Process containment for public native JIT execution.
//!
//! The JIT runtime has process-global state and generated code is not
//! preemptible. Public execution therefore happens in a short-lived worker
//! process, never in the caller. This module deliberately uses only the
//! standard library and libc so the boundary remains small and auditable.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// `flock` is advisory and attaches to an inode. Lock the stable, root-owned
// temporary directory itself: generated Lira code can unlink a user-owned lock
// file and create a fresh inode, but it cannot replace `/tmp`. Ordinary users
// of the directory are unaffected unless they deliberately flock it too.
const LOCK_PATH: &str = "/tmp";
const NATIVE_MEMORY_LIMIT: &str = "268435456";
const NATIVE_MAX_FIBERS: &str = "512";
const MAX_OUTPUT_BYTES: u64 = 8 * 1024 * 1024;
// Keep the public JIT boundary aligned with the bounded integration harness.
pub(crate) const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
const MAX_GROUP_MEMORY_BYTES: u64 = 1024 * 1024 * 1024;
const WALL_LIMIT: Duration = Duration::from_secs(20);
const LOCK_WAIT_LIMIT: Duration = Duration::from_secs(30);
const CPU_LIMIT_SECONDS: u64 = 25;
const GROUP_MEMORY_SNAPSHOT_RETRIES: usize = 4;
const INSPECTION_EXIT_GRACE: Duration = Duration::from_millis(20);
const CLEANUP_WAIT_LIMIT: Duration = Duration::from_millis(250);
static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

#[cfg(unix)]
pub(crate) fn run(worker: &Path, source_file: &str, source: &str) -> Result<i32, String> {
    run_unix(worker, source_file, source)
}

#[cfg(not(unix))]
pub(crate) fn run(_worker: &Path, _source_file: &str, _source: &str) -> Result<i32, String> {
    Err("JIT isolation is unavailable on this platform".to_string())
}

#[cfg(unix)]
fn run_unix(worker: &Path, source_file: &str, source: &str) -> Result<i32, String> {
    use std::os::unix::process::CommandExt;

    if !worker.is_file() {
        return Err(format!(
            "JIT isolation unavailable: worker is not an executable file: {}",
            worker.display()
        ));
    }
    if source.len() > MAX_SOURCE_BYTES {
        return Err(format!(
            "JIT isolation source limit exceeded: {} bytes exceeds {MAX_SOURCE_BYTES}",
            source.len()
        ));
    }
    use std::os::unix::fs::PermissionsExt;
    if worker
        .metadata()
        .map_err(|e| format!("JIT isolation unavailable: inspect worker: {e}"))?
        .permissions()
        .mode()
        & 0o111
        == 0
    {
        return Err(format!(
            "JIT isolation unavailable: worker is not executable: {}",
            worker.display()
        ));
    }

    let execution_lock = ExecutionLock::acquire()?;
    let scratch = ScratchDir::create()?;
    let source_path = scratch.path().join("source.li");
    let stdout_path = scratch.path().join("stdout");
    let stderr_path = scratch.path().join("stderr");
    let result_path = scratch.path().join("result");
    fs::write(&source_path, source.as_bytes())
        .map_err(|e| format!("JIT isolation: write source file: {e}"))?;
    let stdout = File::create(&stdout_path)
        .map_err(|e| format!("JIT isolation: create stdout capture: {e}"))?;
    let stderr = File::create(&stderr_path)
        .map_err(|e| format!("JIT isolation: create stderr capture: {e}"))?;

    let mut command = Command::new(worker);
    command
        .arg("__jit-worker")
        .arg(source_file)
        .arg(&source_path)
        .arg(&result_path)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    // Do not overwrite a caller's deliberately lower limit. The runtime
    // rejects malformed or raised values against its compiled ceilings.
    if std::env::var_os("LIRA_NATIVE_MEMORY_LIMIT_BYTES").is_none() {
        command.env("LIRA_NATIVE_MEMORY_LIMIT_BYTES", NATIVE_MEMORY_LIMIT);
    }
    if std::env::var_os("LIRA_NATIVE_MAX_FIBERS").is_none() {
        command.env("LIRA_NATIVE_MAX_FIBERS", NATIVE_MAX_FIBERS);
    }

    // Establish the process group before exec. Every descendant inherits the
    // group, so cancellation cannot leave a fiber or I/O helper behind.
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            let cpu = libc::rlimit {
                rlim_cur: CPU_LIMIT_SECONDS as libc::rlim_t,
                rlim_max: CPU_LIMIT_SECONDS as libc::rlim_t,
            };
            if libc::setrlimit(libc::RLIMIT_CPU, &cpu) != 0 {
                return Err(io::Error::last_os_error());
            }
            let file_size = libc::rlimit {
                rlim_cur: MAX_OUTPUT_BYTES as libc::rlim_t,
                rlim_max: MAX_OUTPUT_BYTES as libc::rlim_t,
            };
            if libc::setrlimit(libc::RLIMIT_FSIZE, &file_size) != 0 {
                return Err(io::Error::last_os_error());
            }
            #[cfg(target_os = "linux")]
            {
                let address_space = libc::rlimit {
                    rlim_cur: MAX_GROUP_MEMORY_BYTES as libc::rlim_t,
                    rlim_max: MAX_GROUP_MEMORY_BYTES as libc::rlim_t,
                };
                if libc::setrlimit(libc::RLIMIT_AS, &address_space) != 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }

    let mut child = command
        .spawn()
        .map_err(|e| format!("JIT isolation: failed to start worker: {e}"))?;
    let group_id = child.id() as libc::pid_t;

    let status = match monitor_worker(&mut child, group_id, &stdout_path, &stderr_path) {
        Ok(status) => status,
        Err((kind, detail)) => {
            let cleanup = kill_and_reap(&mut child, group_id);
            return Err(match cleanup {
                Ok(()) => format!("JIT isolation {kind}: {detail}"),
                Err(cleanup) => {
                    format!("JIT isolation {kind}: {detail}; cleanup failed: {cleanup}")
                }
            });
        }
    };

    let stdout = read_capped(&stdout_path)
        .map_err(|e| format!("JIT isolation worker failure: read stdout capture: {e}"))?;
    let stderr = read_capped(&stderr_path)
        .map_err(|e| format!("JIT isolation worker failure: read stderr capture: {e}"))?;
    if stdout.len().saturating_add(stderr.len()) > MAX_OUTPUT_BYTES as usize {
        return Err(format!(
            "JIT isolation output limit exceeded: worker output exceeded {MAX_OUTPUT_BYTES} bytes"
        ));
    }
    // Generated execution is complete and its group has been cleaned up. Do
    // not hold the global execution lock while relaying bounded output to a
    // caller-controlled stream that may apply backpressure indefinitely.
    drop(execution_lock);
    relay_output(&stdout, &stderr)?;

    if !status.success() {
        return Err(format!(
            "JIT isolation worker failure: worker exited with {}",
            exit_status_description(status)
        ));
    }
    parse_result(&result_path)
}

#[cfg(unix)]
fn monitor_worker(
    child: &mut Child,
    group_id: libc::pid_t,
    stdout_path: &Path,
    stderr_path: &Path,
) -> Result<ExitStatus, (&'static str, String)> {
    let started = Instant::now();
    loop {
        match child_exit_pending(child) {
            Ok(true) => {
                // The leader can return while a runtime helper is still in
                // the group. Kill descendants while the unreaped leader still
                // pins its PID/process-group identity, then reap it.
                kill_completed_process_group(group_id)
                    .map_err(|error| ("worker cleanup failed", error))?;
                let status = reap_child_bounded(child).map_err(|error| {
                    ("worker failure", format!("reap completed worker: {error}"))
                })?;
                return Ok(status);
            }
            Ok(false) => {}
            Err(error) => return Err(("worker failure", format!("wait failed: {error}"))),
        }

        let mut output_size = 0u64;
        for path in [stdout_path, stderr_path] {
            let size = fs::metadata(path)
                .map_err(|e| ("worker failure", format!("inspect output capture: {e}")))?
                .len();
            output_size = output_size.checked_add(size).ok_or_else(|| {
                (
                    "output limit exceeded",
                    "worker output size overflowed".to_string(),
                )
            })?;
            if output_size > MAX_OUTPUT_BYTES {
                return Err((
                    "output limit exceeded",
                    format!("worker output exceeded {MAX_OUTPUT_BYTES} bytes"),
                ));
            }
        }

        #[cfg(target_os = "macos")]
        {
            let footprint = match mac_group_footprint(group_id) {
                Ok(footprint) => footprint,
                Err(error) => {
                    if let Some(status) = status_after_inspection_race(child)? {
                        return Ok(status);
                    }
                    return Err(("memory limit enforcement failed", error));
                }
            };
            if footprint > MAX_GROUP_MEMORY_BYTES {
                return Err((
                    "memory limit exceeded",
                    format!("worker process group reached {footprint} bytes physical footprint"),
                ));
            }
        }

        #[cfg(target_os = "linux")]
        {
            let resident = match linux_group_resident_bytes(group_id) {
                Ok(resident) => resident,
                Err(error) => {
                    if let Some(status) = status_after_inspection_race(child)? {
                        return Ok(status);
                    }
                    return Err(("memory limit enforcement failed", error));
                }
            };
            if resident > MAX_GROUP_MEMORY_BYTES {
                return Err((
                    "memory limit exceeded",
                    format!("worker process group reached {resident} bytes resident memory"),
                ));
            }
        }

        if started.elapsed() >= WALL_LIMIT {
            return Err((
                "timeout",
                format!("worker exceeded {} seconds", WALL_LIMIT.as_secs()),
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn status_after_inspection_race(
    child: &mut Child,
) -> Result<Option<ExitStatus>, (&'static str, String)> {
    let deadline = Instant::now() + INSPECTION_EXIT_GRACE;
    loop {
        match child_exit_pending(child) {
            Ok(true) => {
                let group_id = child.id() as libc::pid_t;
                kill_completed_process_group(group_id)
                    .map_err(|error| ("worker cleanup failed", error))?;
                let status = reap_child_bounded(child).map_err(|error| {
                    (
                        "worker failure",
                        format!("reap worker after inspection: {error}"),
                    )
                })?;
                return Ok(Some(status));
            }
            Ok(false) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(1));
            }
            Ok(false) => return Ok(None),
            Err(error) => {
                return Err((
                    "worker failure",
                    format!("wait after inspection failed: {error}"),
                ))
            }
        }
    }
}

#[cfg(unix)]
fn child_exit_pending(child: &Child) -> io::Result<bool> {
    loop {
        let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
        // SAFETY: `info` points to writable storage for `siginfo_t`. WNOWAIT
        // observes only this direct child and deliberately leaves it unreaped,
        // which pins the PID while the caller cleans its process group.
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                child.id() as libc::id_t,
                info.as_mut_ptr(),
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result == 0 {
            // SAFETY: successful waitid initialized `info`; a zero si_pid is
            // the specified WNOHANG result when no child status is pending.
            return Ok(unsafe { info.assume_init().si_pid() } != 0);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

#[cfg(unix)]
fn kill_and_reap(child: &mut Child, group_id: libc::pid_t) -> Result<(), String> {
    // A negative pid targets the complete process group. The direct kill is
    // only a race-resistant fallback for a pre-exec/setpgid failure.
    let group_result = kill_process_group(group_id);
    let child_result = child.kill();
    let wait_result = reap_child_bounded(child);
    let mut errors = Vec::new();
    if let Err(error) = group_result {
        errors.push(error);
    }
    if let Err(error) = child_result {
        if error.raw_os_error() != Some(libc::ESRCH) {
            errors.push(format!("kill worker leader: {error}"));
        }
    }
    if let Err(error) = wait_result {
        errors.push(format!("reap worker leader: {error}"));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(unix)]
fn reap_child_bounded(child: &mut Child) -> io::Result<ExitStatus> {
    let deadline = Instant::now() + CLEANUP_WAIT_LIMIT;
    loop {
        if child_exit_pending(child)? {
            return child.wait();
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "timed out waiting to reap cleaned-up worker",
            ));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[cfg(unix)]
fn kill_process_group(group_id: libc::pid_t) -> Result<(), String> {
    if unsafe { libc::kill(-group_id, libc::SIGKILL) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if is_ignorable_group_signal_error(&error) {
        Ok(())
    } else {
        Err(format!("kill worker process group {group_id}: {error}"))
    }
}

#[cfg(unix)]
fn kill_completed_process_group(group_id: libc::pid_t) -> Result<(), String> {
    if unsafe { libc::kill(-group_id, libc::SIGKILL) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH) => Ok(()),
        Some(libc::EPERM) => match completed_group_has_no_descendants(group_id) {
            Ok(true) => Ok(()),
            Ok(false) => Err(format!(
                "kill completed worker process group {group_id}: {error}"
            )),
            Err(inspect_error) => Err(format!(
                "kill completed worker process group {group_id}: {error}; \
                 could not inspect descendants: {inspect_error}"
            )),
        },
        _ => Err(format!(
            "kill completed worker process group {group_id}: {error}"
        )),
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

#[cfg(unix)]
fn is_ignorable_group_signal_error(error: &io::Error) -> bool {
    // ESRCH means the group is already gone. EPERM is a containment failure:
    // there may still be a descendant we were unable to signal.
    error.raw_os_error() == Some(libc::ESRCH)
}

#[cfg(unix)]
fn exit_status_description(status: ExitStatus) -> String {
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    if let Some(signal) = status.signal() {
        format!("signal {signal}")
    } else {
        format!("code {:?}", status.code())
    }
}

#[cfg(unix)]
fn read_capped(path: &Path) -> io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let count = file.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        if bytes.len() as u64 + count as u64 > MAX_OUTPUT_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "captured output exceeds limit",
            ));
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    Ok(bytes)
}

#[cfg(unix)]
fn relay_output(stdout: &[u8], stderr: &[u8]) -> Result<(), String> {
    io::stdout()
        .write_all(stdout)
        .map_err(|e| format!("JIT isolation worker failure: relay stdout: {e}"))?;
    io::stdout()
        .flush()
        .map_err(|e| format!("JIT isolation worker failure: flush stdout: {e}"))?;
    io::stderr()
        .write_all(stderr)
        .map_err(|e| format!("JIT isolation worker failure: relay stderr: {e}"))?;
    io::stderr()
        .flush()
        .map_err(|e| format!("JIT isolation worker failure: flush stderr: {e}"))?;
    Ok(())
}

#[cfg(unix)]
fn parse_result(path: &Path) -> Result<i32, String> {
    let bytes = read_capped(path)
        .map_err(|e| format!("JIT isolation worker failure: result file unavailable: {e}"))?;
    let result = std::str::from_utf8(&bytes)
        .map_err(|_| "JIT isolation worker failure: result file is not UTF-8".to_string())?;
    if let Some(status) = result.strip_prefix("ok:") {
        return status
            .trim()
            .parse::<i32>()
            .map_err(|_| "JIT isolation worker failure: malformed successful result".to_string());
    }
    if let Some(error) = result.strip_prefix("err:") {
        return Err(error.trim_end_matches(['\r', '\n']).to_string());
    }
    Err("JIT isolation worker failure: malformed result file".to_string())
}

#[cfg(unix)]
struct ScratchDir {
    path: PathBuf,
}

#[cfg(unix)]
impl ScratchDir {
    fn create() -> Result<Self, String> {
        use std::os::unix::fs::DirBuilderExt;

        let base = std::env::temp_dir();
        for _ in 0..32 {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default();
            let sequence = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!(
                ".lira-jit-{}-{stamp}-{sequence}",
                std::process::id()
            ));
            match fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!("JIT isolation: create scratch directory: {error}"))
                }
            }
        }
        Err("JIT isolation: could not create a unique scratch directory".to_string())
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(unix)]
impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(unix)]
struct ExecutionLock {
    file: File,
}

#[cfg(unix)]
impl ExecutionLock {
    fn acquire() -> Result<Self, String> {
        use std::os::unix::fs::OpenOptionsExt;
        use std::os::unix::io::AsRawFd;
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY)
            .open(LOCK_PATH)
            .map_err(|e| format!("JIT isolation: open execution lock: {e}"))?;
        let metadata = file
            .metadata()
            .map_err(|e| format!("JIT isolation: inspect execution lock: {e}"))?;
        if !metadata.file_type().is_dir() {
            return Err("JIT isolation: execution lock is not a directory".to_string());
        }
        let started = Instant::now();
        loop {
            let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if result == 0 {
                return Ok(Self { file });
            }
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            if error.kind() == io::ErrorKind::WouldBlock {
                if started.elapsed() >= LOCK_WAIT_LIMIT {
                    return Err(format!(
                        "JIT isolation: execution lock was unavailable for {} seconds",
                        LOCK_WAIT_LIMIT.as_secs()
                    ));
                }
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            return Err(format!("JIT isolation: acquire execution lock: {error}"));
        }
    }
}

#[cfg(unix)]
impl Drop for ExecutionLock {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;
        unsafe {
            let _ = libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

/// Retry a complete group snapshot when a member exits or its proc entry
/// disappears after enumeration.
///
/// The caller must make each attempt enumerate and measure the whole group
/// anew: keeping a partial total after `ESRCH` or `NotFound` would undercount
/// the group and could let an allocation spike escape its containment limit.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
fn retry_complete_group_snapshot<T>(
    attempts: usize,
    mut snapshot: impl FnMut() -> io::Result<T>,
) -> io::Result<T> {
    debug_assert!(attempts > 0);
    for attempt in 0..attempts {
        match snapshot() {
            Ok(value) => return Ok(value),
            Err(error) if is_transient_group_snapshot_error(&error) && attempt + 1 < attempts => {
                // A descendant vanished between enumeration and sampling.
                // Yield before beginning a fresh, complete snapshot.
                std::thread::yield_now();
            }
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other(
        "process-group memory sampling retry count was zero",
    ))
}

#[cfg(any(target_os = "macos", target_os = "linux", test))]
fn is_transient_group_snapshot_error(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound || error.raw_os_error() == Some(libc::ESRCH)
}

#[cfg(target_os = "macos")]
fn mac_group_footprint(group_id: libc::pid_t) -> Result<u64, String> {
    retry_complete_group_snapshot(GROUP_MEMORY_SNAPSHOT_RETRIES, || {
        mac_group_footprint_once(group_id)
    })
    .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn mac_group_footprint_once(group_id: libc::pid_t) -> io::Result<u64> {
    #[repr(C)]
    struct RusageInfoV2 {
        bytes: [u8; 160],
    }
    unsafe extern "C" {
        fn proc_listpgrppids(
            pgrpid: libc::pid_t,
            buffer: *mut libc::c_void,
            buffersize: libc::c_int,
        ) -> libc::c_int;
        fn proc_pid_rusage(
            pid: libc::pid_t,
            flavor: libc::c_int,
            buffer: *mut libc::c_void,
        ) -> libc::c_int;
    }

    let mut capacity = 32usize;
    let members = loop {
        let mut pids = vec![0 as libc::pid_t; capacity];
        let count = unsafe {
            proc_listpgrppids(
                group_id,
                pids.as_mut_ptr().cast(),
                (capacity * std::mem::size_of::<libc::pid_t>()) as libc::c_int,
            )
        };
        if count < 0 {
            return Err(io::Error::last_os_error());
        }
        // `proc_listpgrppids` divides the byte result from `proc_listpids`
        // by `sizeof(pid_t)` and therefore returns a PID count.
        let count = count as usize;
        if count > capacity {
            return Err(io::Error::other(
                "process group enumeration exceeded its buffer",
            ));
        }
        if count < capacity {
            pids.truncate(count);
            break pids;
        }
        capacity = capacity
            .checked_mul(2)
            .ok_or_else(|| io::Error::other("process group enumeration overflowed"))?;
        if capacity > 4096 {
            return Err(io::Error::other("process group has too many members"));
        }
    };
    if members.is_empty() {
        return Err(io::Error::other(
            "process group enumeration returned no live members",
        ));
    }

    let mut total = 0u64;
    for pid in members {
        if pid <= 0 {
            return Err(io::Error::other(
                "process group enumeration returned an invalid pid",
            ));
        }
        let mut info = RusageInfoV2 { bytes: [0; 160] };
        let result = unsafe {
            proc_pid_rusage(
                pid,
                2,
                (&mut info as *mut RusageInfoV2).cast::<libc::c_void>(),
            )
        };
        if result != 0 {
            // A listed descendant may exit before it can be sampled. The
            // outer retry discards this partial total and restarts the full
            // group snapshot only for that transient ESRCH case.
            return Err(io::Error::last_os_error());
        }
        let footprint = u64::from_ne_bytes(
            info.bytes[72..80]
                .try_into()
                .map_err(|_| io::Error::other("invalid physical footprint field"))?,
        );
        total = total
            .checked_add(footprint)
            .ok_or_else(|| io::Error::other("process group footprint overflowed"))?;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_snapshot_retries_a_transient_member_exit() {
        let mut calls = 0;
        let footprint = retry_complete_group_snapshot(4, || {
            calls += 1;
            if calls < 3 {
                Err(io::Error::from_raw_os_error(libc::ESRCH))
            } else {
                Ok(42_u64)
            }
        })
        .expect("third complete snapshot should succeed");

        assert_eq!(footprint, 42);
        assert_eq!(calls, 3);
    }

    #[test]
    fn group_snapshot_retries_a_disappearing_member() {
        let mut calls = 0;
        let footprint = retry_complete_group_snapshot(4, || {
            calls += 1;
            if calls == 1 {
                Err(io::Error::from(io::ErrorKind::NotFound))
            } else {
                Ok(42_u64)
            }
        })
        .expect("second complete snapshot should succeed");

        assert_eq!(footprint, 42);
        assert_eq!(calls, 2);
    }

    #[test]
    fn group_snapshot_retries_are_bounded_and_fail_closed() {
        let mut calls = 0;
        let error = retry_complete_group_snapshot::<()>(4, || {
            calls += 1;
            Err(io::Error::from_raw_os_error(libc::ESRCH))
        })
        .expect_err("persistent member exits must not produce a partial snapshot");

        assert_eq!(calls, 4);
        assert_eq!(error.raw_os_error(), Some(libc::ESRCH));
    }

    #[test]
    fn group_snapshot_not_found_retries_are_bounded_and_fail_closed() {
        let mut calls = 0;
        let error = retry_complete_group_snapshot::<()>(4, || {
            calls += 1;
            Err(io::Error::from(io::ErrorKind::NotFound))
        })
        .expect_err("persistent proc entry disappearance must fail closed");

        assert_eq!(calls, 4);
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn group_snapshot_does_not_retry_non_transient_errors() {
        let mut calls = 0;
        let error = retry_complete_group_snapshot::<()>(4, || {
            calls += 1;
            Err(io::Error::from_raw_os_error(libc::EPERM))
        })
        .expect_err("permission failures must remain containment failures");

        assert_eq!(calls, 1);
        assert_eq!(error.raw_os_error(), Some(libc::EPERM));
    }

    #[test]
    fn cleanup_only_ignores_a_gone_process_group() {
        assert!(is_ignorable_group_signal_error(
            &io::Error::from_raw_os_error(libc::ESRCH,)
        ));
        assert!(!is_ignorable_group_signal_error(
            &io::Error::from_raw_os_error(libc::EPERM,)
        ));
    }
}

#[cfg(target_os = "linux")]
fn linux_group_resident_bytes(group_id: libc::pid_t) -> Result<u64, String> {
    retry_complete_group_snapshot(GROUP_MEMORY_SNAPSHOT_RETRIES, || {
        linux_group_resident_bytes_once(group_id)
    })
    .map_err(|error| error.to_string())
}

#[cfg(target_os = "linux")]
fn linux_group_resident_bytes_once(group_id: libc::pid_t) -> io::Result<u64> {
    let group_id = group_id.to_string();
    let mut total = 0u64;
    let mut found = false;
    for entry in fs::read_dir("/proc")? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(pid) = name
            .to_str()
            .filter(|name| name.bytes().all(|b| b.is_ascii_digit()))
        else {
            continue;
        };
        let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
        let Some(comm_end) = stat.rfind(')') else {
            return Err(io::Error::other(format!("malformed /proc/{pid}/stat")));
        };
        let fields: Vec<&str> = stat[comm_end + 1..].split_whitespace().collect();
        if fields.get(2).copied() != Some(group_id.as_str()) {
            continue;
        }
        found = true;
        let status = fs::read_to_string(format!("/proc/{pid}/status"))?;
        let resident_kib = status
            .lines()
            .find_map(|line| line.strip_prefix("VmRSS:")?.split_whitespace().next())
            .ok_or_else(|| io::Error::other(format!("/proc/{pid}/status has no VmRSS")))?
            .parse::<u64>()
            .map_err(|e| io::Error::other(format!("invalid VmRSS for {pid}: {e}")))?;
        total = total
            .checked_add(resident_kib.saturating_mul(1024))
            .ok_or_else(|| io::Error::other("process group resident memory overflowed"))?;
    }
    if !found {
        return Err(io::Error::other(
            "process group enumeration returned no live members",
        ));
    }
    Ok(total)
}
