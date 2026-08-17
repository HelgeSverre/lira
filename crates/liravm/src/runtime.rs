//! Lira Runtime
//!
//! Built-in functions and system call bindings.
//! Maps to native syscalls on macOS/Linux.
//!
//! Syscall numbers:
//! - 0: sys_exit (handled directly in VM)
//! - 1: sys_print
//! - 2: sys_println
//! - 3: sys_read_line
//! - 4: sys_time_ms
//! - 5: sys_sleep_ms
//! - 6: time_secs (current time in seconds since epoch)
//! - 7: time_micros (current time in microseconds)
//! - 8: time_nanos (current time in nanoseconds)
//! - 10: file_open
//! - 11: file_read
//! - 12: file_write
//! - 13: file_close
//! - 14: file_exists
//! - 15: file_size
//! - 20: env_get
//! - 21: env_args
//! - 200: env_set (set environment variable)
//! - 201: env_remove (remove environment variable)
//! - 202: env_all (get all env vars as key=value pairs)
//! - 203: env_keys (get all env var names)
//! - 204: env_has (check if env var exists)
//! - 205: env_exe (get executable path)
//! - 206: env_temp_dir (get temp directory)
//! - 207: env_home_dir (get home directory)
//! - 30: str_char_code (get char code at index)
//! - 31: str_from_char_code (create string from char code)
//! - 32: str_to_upper (convert to uppercase)
//! - 33: str_to_lower (convert to lowercase)
//! - 34: str_substring (get substring)
//! - 35: str_index_of (find first occurrence)
//! - 36: str_split (split by delimiter)
//! - 37: str_trim (trim whitespace)
//! - 40: random_float
//! - 41: random_int
//! - 50: json_parse (parse JSON string to value)
//! - 51: json_stringify (stringify value to JSON)
//! - 52: json_stringify_pretty (stringify with pretty printing)
//! - 60: base64_encode (encode string to base64)
//! - 61: base64_decode (decode base64 to string)
//! - 62: base64_encode_url (URL-safe base64 encode)
//! - 63: base64_decode_url (URL-safe base64 decode)
//! - 70: md5 (compute MD5 hash)
//! - 71: sha1 (compute SHA1 hash)
//! - 72: sha256 (compute SHA256 hash)
//! - 73: sha512 (compute SHA512 hash)
//! - 80: tcp_connect (connect to TCP server)
//! - 81: tcp_write (write data to socket)
//! - 82: tcp_read (read data from socket)
//! - 83: tcp_close (close socket)
//! - 84: dns_lookup (resolve hostname to IP)
//! - 90: getcwd (get current working directory)
//! - 91: chdir (change directory)
//! - 92: mkdir (create directory)
//! - 93: mkdir_all (create directory with parents)
//! - 94: rmdir (remove empty directory)
//! - 95: remove (remove file)
//! - 96: remove_all (remove directory tree)
//! - 97: listdir (list directory contents)
//! - 98: is_dir (check if path is directory)
//! - 99: is_file (check if path is file)
//! - 100: rename (rename/move file or directory)
//! - 101: copy (copy file)
//! - 110: url_encode (URL percent-encode string)
//! - 111: url_decode (URL percent-decode string)
//! - 130: time_format_iso (format timestamp as ISO 8601 string)
//! - 131: time_format (format timestamp with custom format)
//! - 132: time_parse_iso (parse ISO 8601 string to timestamp)
//! - 133: time_timezone_offset (get local timezone offset in minutes)
//! - 134: time_components (get date components from timestamp)
//! - 135: time_from_components (create timestamp from components)
//! - 120: http_get (HTTP GET request)
//! - 121: http_post (HTTP POST request)
//! - 122: http_request (HTTP request with custom method)
//! - 140: math_sqrt (square root)
//! - 141: math_pow (power)
//! - 142: math_exp (exponential)
//! - 143: math_ln (natural log)
//! - 144: math_log10 (log base 10)
//! - 145: math_log2 (log base 2)
//! - 146: math_sin (sine)
//! - 147: math_cos (cosine)
//! - 148: math_tan (tangent)
//! - 149: math_asin (arcsine)
//! - 150: math_acos (arccosine)
//! - 151: math_atan (arctangent)
//! - 152: math_atan2 (two-argument arctangent)
//! - 153: math_sinh (hyperbolic sine)
//! - 154: math_cosh (hyperbolic cosine)
//! - 155: math_tanh (hyperbolic tangent)
//! - 156: math_floor (round down)
//! - 157: math_ceil (round up)
//! - 158: math_round (round to nearest)
//! - 159: math_trunc (truncate)
//! - 160: math_is_nan (check if NaN)
//! - 161: math_is_infinite (check if infinite)
//! - 162: math_is_finite (check if finite)
//! - 163: math_abs (absolute value for floats)
//! - 170: regex_match (check if pattern matches string)
//! - 171: regex_find (find first match)
//! - 172: regex_find_all (find all matches)
//! - 173: regex_replace (replace first occurrence)
//! - 174: regex_replace_all (replace all occurrences)
//! - 175: regex_split (split by pattern)
//! - 176: regex_captures (get capture groups)
//! - 177: regex_is_valid (check if pattern is valid)
//! - 190: uuid_v4 (generate random UUID v4)
//! - 191: uuid_v7 (generate time-ordered UUID v7)
//! - 192: uuid_is_valid (check if string is valid UUID)
//! - 193: uuid_nil (get nil UUID)

use crate::value::Value;
use gc::{Gc, GcCell};
use md5::{Digest, Md5};
use regex::{Captures, Regex, RegexBuilder};
use serde_json::{Map, Value as JsonValue};
use sha1::Sha1;
use sha2::{Sha256, Sha512};
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::TcpStream;
use std::rc::Rc;
use std::time::Duration;

/// File handle for open files
pub type FileHandle = i64;

const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
const MAX_JSON_NODES: usize = 1_000_000;
const MAX_JSON_DEPTH: usize = 128;
const MAX_JSON_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
const JSON_SIZE_LIMIT_ERROR: &str = "input exceeds JSON size limit";
const JSON_RESOURCE_LIMIT_ERROR: &str = "input exceeds JSON resource limit";

// Keep regex work bounded in both the VM and native runtimes. These limits
// prevent one call from consuming unaccounted compilation, iteration, or
// output memory; the native runtime mirrors these exact values.
const MAX_REGEX_PATTERN_BYTES: usize = 64 * 1024;
const MAX_REGEX_INPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_REGEX_RESULT_COUNT: usize = 100_000;
const MAX_REGEX_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
const MAX_REGEX_COMPILED_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JsonResourceError {
    Size,
    Resource,
}

#[derive(Clone, Copy)]
enum JsonFrame {
    ArrayValue,
    ArrayAfterValue,
    ObjectKey,
    ObjectColon,
    ObjectValue,
    ObjectAfterValue,
}

fn skip_json_string(bytes: &[u8], mut index: usize) -> usize {
    index += 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index = index.saturating_add(2),
            b'"' => return index + 1,
            _ => index += 1,
        }
    }
    bytes.len()
}

fn skip_json_primitive(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len()
        && !bytes[index].is_ascii_whitespace()
        && !matches!(bytes[index], b',' | b']' | b'}')
    {
        index += 1;
    }
    index
}

/// Preflight JSON without using serde_json's recursive parser. This keeps
/// deeply nested adversarial input from reaching the parser before our depth
/// limit is known, while serde_json remains responsible for syntax validation.
fn check_json_resources(input: &str) -> Result<(), JsonResourceError> {
    if input.len() > MAX_JSON_BYTES {
        return Err(JsonResourceError::Size);
    }
    let bytes = input.as_bytes();
    let mut frames = Vec::new();
    let mut root_done = false;
    let mut nodes = 0usize;
    let mut index = 0usize;

    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index == bytes.len() {
            break;
        }

        let byte = bytes[index];
        match frames.last_mut().copied() {
            Some(JsonFrame::ArrayAfterValue) if byte == b',' => {
                if let Some(frame) = frames.last_mut() {
                    *frame = JsonFrame::ArrayValue;
                }
                index += 1;
            }
            Some(JsonFrame::ArrayAfterValue) if byte == b']' => {
                frames.pop();
                index += 1;
            }
            Some(JsonFrame::ArrayValue) if byte == b']' => {
                frames.pop();
                index += 1;
            }
            Some(JsonFrame::ArrayValue) => {
                let depth = frames.len();
                if depth > MAX_JSON_DEPTH || nodes >= MAX_JSON_NODES {
                    return Err(JsonResourceError::Resource);
                }
                nodes += 1;
                if let Some(frame) = frames.last_mut() {
                    *frame = JsonFrame::ArrayAfterValue;
                }
                if matches!(byte, b'[' | b'{') {
                    frames.push(if byte == b'[' {
                        JsonFrame::ArrayValue
                    } else {
                        JsonFrame::ObjectKey
                    });
                    index += 1;
                } else if byte == b'"' {
                    index = skip_json_string(bytes, index);
                } else {
                    index = skip_json_primitive(bytes, index);
                }
            }
            Some(JsonFrame::ObjectKey) if byte == b'}' => {
                frames.pop();
                index += 1;
            }
            Some(JsonFrame::ObjectKey) if byte == b'"' => {
                index = skip_json_string(bytes, index);
                if let Some(frame) = frames.last_mut() {
                    *frame = JsonFrame::ObjectColon;
                }
            }
            Some(JsonFrame::ObjectColon) if byte == b':' => {
                if let Some(frame) = frames.last_mut() {
                    *frame = JsonFrame::ObjectValue;
                }
                index += 1;
            }
            Some(JsonFrame::ObjectValue) => {
                let depth = frames.len();
                if depth > MAX_JSON_DEPTH || nodes >= MAX_JSON_NODES {
                    return Err(JsonResourceError::Resource);
                }
                nodes += 1;
                if let Some(frame) = frames.last_mut() {
                    *frame = JsonFrame::ObjectAfterValue;
                }
                if matches!(byte, b'[' | b'{') {
                    frames.push(if byte == b'[' {
                        JsonFrame::ArrayValue
                    } else {
                        JsonFrame::ObjectKey
                    });
                    index += 1;
                } else if byte == b'"' {
                    index = skip_json_string(bytes, index);
                } else {
                    index = skip_json_primitive(bytes, index);
                }
            }
            Some(JsonFrame::ObjectAfterValue) if byte == b',' => {
                if let Some(frame) = frames.last_mut() {
                    *frame = JsonFrame::ObjectKey;
                }
                index += 1;
            }
            Some(JsonFrame::ObjectAfterValue) if byte == b'}' => {
                frames.pop();
                index += 1;
            }
            Some(_) => {
                // Let serde_json report malformed punctuation. Advancing here
                // still ensures malformed input cannot make this preflight loop.
                index += 1;
            }
            None if root_done => break,
            None => {
                if nodes >= MAX_JSON_NODES {
                    return Err(JsonResourceError::Resource);
                }
                nodes += 1;
                root_done = true;
                if matches!(byte, b'[' | b'{') {
                    frames.push(if byte == b'[' {
                        JsonFrame::ArrayValue
                    } else {
                        JsonFrame::ObjectKey
                    });
                    index += 1;
                } else if byte == b'"' {
                    index = skip_json_string(bytes, index);
                } else {
                    index = skip_json_primitive(bytes, index);
                }
            }
        }
    }
    Ok(())
}

struct JsonBuildState {
    nodes: usize,
}

impl JsonBuildState {
    fn visit(&mut self, depth: usize) -> Result<(), String> {
        if depth > MAX_JSON_DEPTH || self.nodes >= MAX_JSON_NODES {
            return Err(JSON_RESOURCE_LIMIT_ERROR.to_owned());
        }
        self.nodes += 1;
        Ok(())
    }
}

#[derive(Debug)]
enum JsonConvertError {
    ResourceLimit,
}

struct JsonConvertState {
    nodes: usize,
    active: HashSet<usize>,
}

impl JsonConvertState {
    fn visit(&mut self, depth: usize) -> Result<(), JsonConvertError> {
        if depth > MAX_JSON_DEPTH || self.nodes >= MAX_JSON_NODES {
            return Err(JsonConvertError::ResourceLimit);
        }
        self.nodes += 1;
        Ok(())
    }
}

fn build_regex(pattern: &str) -> Option<Regex> {
    if pattern.len() > MAX_REGEX_PATTERN_BYTES {
        return None;
    }
    RegexBuilder::new(pattern)
        .size_limit(MAX_REGEX_COMPILED_BYTES)
        .dfa_size_limit(MAX_REGEX_COMPILED_BYTES)
        .build()
        .ok()
}

fn valid_regex_input(pattern: &str, text: &str) -> bool {
    pattern.len() <= MAX_REGEX_PATTERN_BYTES && text.len() <= MAX_REGEX_INPUT_BYTES
}

fn push_regex_bounded(output: &mut String, value: &str) -> bool {
    let Some(next_len) = output.len().checked_add(value.len()) else {
        return false;
    };
    if next_len > MAX_REGEX_OUTPUT_BYTES {
        return false;
    }
    output.push_str(value);
    true
}

fn regex_capture_reference<'a>(captures: &'a Captures<'_>, reference: &str) -> Option<&'a str> {
    if let Ok(index) = reference.parse::<usize>() {
        return captures.get(index).map(|capture| capture.as_str());
    }
    captures.name(reference).map(|capture| capture.as_str())
}

/// Append a replacement with regex's `$name`, `${name}`, `$0`, and `$$`
/// interpolation semantics without giving the interpolator an unbounded
/// destination string.
fn push_regex_replacement(captures: &Captures<'_>, replacement: &str, output: &mut String) -> bool {
    let mut remaining = replacement;
    while !remaining.is_empty() {
        let Some(dollar) = remaining.find('$') else {
            return push_regex_bounded(output, remaining);
        };
        if !push_regex_bounded(output, &remaining[..dollar]) {
            return false;
        }
        remaining = &remaining[dollar..];
        if remaining.as_bytes().get(1) == Some(&b'$') {
            if !push_regex_bounded(output, "$") {
                return false;
            }
            remaining = &remaining[2..];
            continue;
        }

        let bytes = remaining.as_bytes();
        let (reference, end) = if bytes.get(1) == Some(&b'{') {
            let Some(close) = remaining[2..].find('}') else {
                if !push_regex_bounded(output, "$") {
                    return false;
                }
                remaining = &remaining[1..];
                continue;
            };
            let end = close + 3;
            (&remaining[2..end - 1], end)
        } else {
            let mut end = 1;
            while bytes
                .get(end)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            {
                end += 1;
            }
            if end == 1 {
                if !push_regex_bounded(output, "$") {
                    return false;
                }
                remaining = &remaining[1..];
                continue;
            }
            (&remaining[1..end], end)
        };
        if let Some(value) = regex_capture_reference(captures, reference) {
            if !push_regex_bounded(output, value) {
                return false;
            }
        }
        remaining = &remaining[end..];
    }
    true
}

fn bounded_regex_replace(
    regex: &Regex,
    text: &str,
    replacement: &str,
    all: bool,
) -> Option<String> {
    let mut output = String::new();
    let mut last_match = 0;
    for (match_count, captures) in regex.captures_iter(text).enumerate() {
        if match_count >= MAX_REGEX_RESULT_COUNT {
            return None;
        }
        let full_match = captures.get(0)?;
        if !push_regex_bounded(&mut output, &text[last_match..full_match.start()])
            || !push_regex_replacement(&captures, replacement, &mut output)
        {
            return None;
        }
        last_match = full_match.end();
        if !all {
            break;
        }
    }
    if !push_regex_bounded(&mut output, &text[last_match..]) {
        return None;
    }
    Some(output)
}

/// Runtime context for built-in function calls
pub struct Runtime {
    /// Standard output buffer
    #[allow(dead_code)]
    stdout: String,
    /// Open file handles (fd -> File)
    files: HashMap<FileHandle, File>,
    /// Next file descriptor to assign
    next_fd: FileHandle,
    /// Open TCP sockets (socket_id -> TcpStream)
    tcp_sockets: HashMap<i64, TcpStream>,
    /// Next socket ID to assign
    next_socket_id: i64,
    /// Per-VM environment values used by hermetic embedders and tests.
    env_overrides: HashMap<String, String>,
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            stdout: String::new(),
            files: HashMap::new(),
            next_fd: 10, // Start at 10 to avoid confusion with stdin/stdout/stderr
            tcp_sockets: HashMap::new(),
            next_socket_id: 100, // Start at 100 to distinguish from file handles
            env_overrides: HashMap::new(),
        }
    }

    /// Print a value to stdout
    pub fn print(&mut self, value: &Value) {
        print!("{}", value);
    }

    /// Print a value with newline
    pub fn println(&mut self, value: &Value) {
        self.print(value);
        println!();
    }

    /// Read a line from stdin
    pub fn read_line(&self) -> Result<String, String> {
        let mut line = String::new();
        std::io::stdin()
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        Ok(line.trim_end().to_string())
    }

    /// Get current time in milliseconds
    pub fn current_time_millis(&self) -> i64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    /// Sleep for the given number of milliseconds
    pub fn sleep(&self, millis: i64) {
        if millis > 0 {
            std::thread::sleep(std::time::Duration::from_millis(millis as u64));
        }
    }

    /// Get current time in seconds since epoch
    pub fn current_time_secs(&self) -> i64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    /// Get current time in microseconds since epoch
    pub fn current_time_micros(&self) -> i64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_micros() as i64)
            .unwrap_or(0)
    }

    /// Get current time in nanoseconds since epoch
    pub fn current_time_nanos(&self) -> i64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as i64)
            .unwrap_or(0)
    }

    /// Format timestamp as ISO 8601 string
    pub fn time_format_iso(&self, timestamp_ms: i64) -> String {
        use chrono::{DateTime, Utc};
        let secs = timestamp_ms / 1000;
        let nsecs = ((timestamp_ms % 1000) * 1_000_000) as u32;
        DateTime::<Utc>::from_timestamp(secs, nsecs)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_default()
    }

    /// Format timestamp with custom format string
    pub fn time_format(&self, timestamp_ms: i64, format: &str) -> String {
        use chrono::{DateTime, Utc};
        let secs = timestamp_ms / 1000;
        let nsecs = ((timestamp_ms % 1000) * 1_000_000) as u32;
        DateTime::<Utc>::from_timestamp(secs, nsecs)
            .map(|dt| dt.format(format).to_string())
            .unwrap_or_default()
    }

    /// Parse ISO 8601 string to timestamp in milliseconds
    pub fn time_parse_iso(&self, s: &str) -> i64 {
        use chrono::DateTime;
        DateTime::parse_from_rfc3339(s)
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(0)
    }

    /// Get local timezone offset in minutes from UTC
    pub fn time_timezone_offset(&self) -> i64 {
        use chrono::Local;
        Local::now().offset().local_minus_utc() as i64 / 60
    }

    /// Get date components from timestamp (year, month, day, hour, minute, second)
    pub fn time_components(&self, timestamp_ms: i64) -> Vec<i64> {
        use chrono::{DateTime, Datelike, Timelike, Utc};
        let secs = timestamp_ms / 1000;
        let nsecs = ((timestamp_ms % 1000) * 1_000_000) as u32;
        DateTime::<Utc>::from_timestamp(secs, nsecs)
            .map(|dt| {
                vec![
                    dt.year() as i64,
                    dt.month() as i64,
                    dt.day() as i64,
                    dt.hour() as i64,
                    dt.minute() as i64,
                    dt.second() as i64,
                ]
            })
            .unwrap_or_else(|| vec![0; 6])
    }

    /// Create timestamp from date components (year, month, day, hour, minute, second)
    pub fn time_from_components(
        &self,
        year: i64,
        month: i64,
        day: i64,
        hour: i64,
        min: i64,
        sec: i64,
    ) -> i64 {
        use chrono::{TimeZone, Utc};
        // Fail closed (return 0, matching a `timegm` failure and the native
        // backend) rather than truncating a component that cannot be held by
        // the underlying narrow chrono types. Without this, an extreme `year`
        // (e.g. i64::MIN) would silently become a plausible but wrong date.
        let Some(year) = i32::try_from(year).ok() else {
            return 0;
        };
        let Some(month) = u32::try_from(month).ok() else {
            return 0;
        };
        let Some(day) = u32::try_from(day).ok() else {
            return 0;
        };
        let Some(hour) = u32::try_from(hour).ok() else {
            return 0;
        };
        let Some(min) = u32::try_from(min).ok() else {
            return 0;
        };
        let Some(sec) = u32::try_from(sec).ok() else {
            return 0;
        };
        Utc.with_ymd_and_hms(year, month, day, hour, min, sec)
            .single()
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(0)
    }

    // ========================================================================
    // Handle registry access for the I/O offload pool
    //
    // These let the VM thread *check out* a File/TcpStream (remove it from the
    // registry), move it into a pool job, and re-insert it when the job
    // completes — all on the VM thread, so the registries stay lock-free.
    // ========================================================================

    /// Reserve the next file handle id without opening anything (the open runs
    /// on a pool thread, but ids must be allocated on the VM thread to stay
    /// monotonic regardless of completion order).
    pub(crate) fn alloc_fd(&mut self) -> FileHandle {
        let fd = self.next_fd;
        self.next_fd += 1;
        fd
    }

    /// Remove a file from the registry so it can be moved to a pool thread.
    /// `None` if the fd is unknown or already checked out (in flight).
    pub(crate) fn checkout_file(&mut self, fd: FileHandle) -> Option<File> {
        self.files.remove(&fd)
    }

    pub(crate) fn insert_file(&mut self, fd: FileHandle, file: File) {
        self.files.insert(fd, file);
    }

    pub(crate) fn alloc_socket_id(&mut self) -> i64 {
        let id = self.next_socket_id;
        self.next_socket_id += 1;
        id
    }

    pub(crate) fn checkout_socket(&mut self, id: i64) -> Option<TcpStream> {
        self.tcp_sockets.remove(&id)
    }

    pub(crate) fn insert_socket(&mut self, id: i64, stream: TcpStream) {
        self.tcp_sockets.insert(id, stream);
    }

    // ========================================================================
    // File I/O Operations
    // ========================================================================

    /// Open a file (the blocking part), with no `&self` so it can run on a pool
    /// thread. Returns the owned `File`; the caller allocates the fd and inserts.
    pub(crate) fn open_file_blocking(path: &str, mode: i64) -> Result<File, String> {
        let file = match mode {
            0 => File::open(path), // read only
            1 => OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(path), // write (create/truncate)
            2 => OpenOptions::new().append(true).create(true).open(path), // append
            3 => OpenOptions::new().read(true).write(true).open(path), // read+write
            _ => return Err(format!("Invalid file mode: {}", mode)),
        };
        file.map_err(|e| format!("Failed to open '{}': {}", path, e))
    }

    /// Read up to `max_bytes` from an owned file (checked out of the registry).
    /// Usable off the VM thread.
    pub(crate) fn read_file_blocking(file: &mut File, max_bytes: i64) -> Result<String, String> {
        let mut buffer = vec![0u8; max_bytes.clamp(0, 1024 * 1024) as usize]; // cap at 1MB
        let bytes_read = file
            .read(&mut buffer)
            .map_err(|e| format!("Read error: {}", e))?;
        buffer.truncate(bytes_read);
        String::from_utf8(buffer).map_err(|e| format!("UTF-8 decode error: {}", e))
    }

    /// Write to an owned file (checked out of the registry). Usable off the VM
    /// thread.
    pub(crate) fn write_file_blocking(file: &mut File, data: &str) -> Result<i64, String> {
        file.write(data.as_bytes())
            .map(|n| n as i64)
            .map_err(|e| format!("Write error: {}", e))
    }

    /// Open a file and return a handle
    /// mode: 0 = read, 1 = write, 2 = append, 3 = read+write
    pub fn file_open(&mut self, path: &str, mode: i64) -> Result<FileHandle, String> {
        let file = Self::open_file_blocking(path, mode)?;
        let fd = self.next_fd;
        self.next_fd += 1;
        self.files.insert(fd, file);
        Ok(fd)
    }

    /// Read bytes from file into a string (up to max_bytes)
    pub fn file_read(&mut self, fd: FileHandle, max_bytes: i64) -> Result<String, String> {
        let file = self
            .files
            .get_mut(&fd)
            .ok_or_else(|| format!("Invalid file handle: {}", fd))?;

        let mut buffer = vec![0u8; max_bytes.clamp(0, 1024 * 1024) as usize]; // Cap at 1MB
        let bytes_read = file
            .read(&mut buffer)
            .map_err(|e| format!("Read error: {}", e))?;

        buffer.truncate(bytes_read);
        String::from_utf8(buffer).map_err(|e| format!("UTF-8 decode error: {}", e))
    }

    /// Write a string to a file, return bytes written
    pub fn file_write(&mut self, fd: FileHandle, data: &str) -> Result<i64, String> {
        let file = self
            .files
            .get_mut(&fd)
            .ok_or_else(|| format!("Invalid file handle: {}", fd))?;

        let bytes_written = file
            .write(data.as_bytes())
            .map_err(|e| format!("Write error: {}", e))?;

        Ok(bytes_written as i64)
    }

    /// Close a file handle
    pub fn file_close(&mut self, fd: FileHandle) -> Result<(), String> {
        self.files
            .remove(&fd)
            .ok_or_else(|| format!("Invalid file handle: {}", fd))?;
        Ok(())
    }

    /// Check if a file exists
    pub fn file_exists(&self, path: &str) -> bool {
        std::path::Path::new(path).exists()
    }

    /// Get file size in bytes
    pub fn file_size(&self, path: &str) -> Result<i64, String> {
        std::fs::metadata(path)
            .map(|m| m.len() as i64)
            .map_err(|e| format!("Failed to get file size: {}", e))
    }

    /// Seek in file, return new position
    pub fn file_seek(&mut self, fd: FileHandle, offset: i64, whence: i64) -> Result<i64, String> {
        let file = self
            .files
            .get_mut(&fd)
            .ok_or_else(|| format!("Invalid file handle: {}", fd))?;

        let pos = match whence {
            0 => SeekFrom::Start(offset as u64), // SEEK_SET
            1 => SeekFrom::Current(offset),      // SEEK_CUR
            2 => SeekFrom::End(offset),          // SEEK_END
            _ => return Err(format!("Invalid seek whence: {}", whence)),
        };

        file.seek(pos)
            .map(|p| p as i64)
            .map_err(|e| format!("Seek error: {}", e))
    }

    // ========================================================================
    // Environment Operations
    // ========================================================================

    /// Get an environment variable
    pub fn env_get(&self, name: &str) -> Option<String> {
        self.env_overrides
            .get(name)
            .cloned()
            .or_else(|| std::env::var(name).ok())
    }

    /// Override an environment value for this runtime instance only.
    pub(crate) fn set_env_override(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.env_overrides.insert(name.into(), value.into());
    }

    /// Get command line arguments
    pub fn env_args(&self) -> Vec<String> {
        std::env::args().collect()
    }

    /// Set environment variable
    pub fn env_set(&self, name: &str, value: &str) -> bool {
        // SAFETY: We accept the risk of data races in this single-threaded VM context
        unsafe { std::env::set_var(name, value) };
        true
    }

    /// Remove environment variable
    pub fn env_remove(&self, name: &str) -> bool {
        // SAFETY: We accept the risk of data races in this single-threaded VM context
        unsafe { std::env::remove_var(name) };
        true
    }

    /// Get all environment variables as key=value pairs
    pub fn env_all(&self) -> Vec<String> {
        std::env::vars()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect()
    }

    /// Get all environment variable names
    pub fn env_keys(&self) -> Vec<String> {
        std::env::vars().map(|(k, _)| k).collect()
    }

    /// Check if environment variable exists
    pub fn env_has(&self, name: &str) -> bool {
        std::env::var(name).is_ok()
    }

    /// Get executable path
    pub fn env_exe(&self) -> String {
        std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    /// Get temp directory
    pub fn env_temp_dir(&self) -> String {
        std::env::temp_dir().to_string_lossy().to_string()
    }

    /// Get home directory
    pub fn env_home_dir(&self) -> String {
        dirs::home_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    // ========================================================================
    // Random Number Generation
    // ========================================================================

    /// Generate random bytes using the OS CSPRNG
    pub fn random_bytes(&self, count: usize) -> Vec<u8> {
        let mut buf = vec![0u8; count];
        getrandom::getrandom(&mut buf).expect("Failed to get random bytes from OS");
        buf
    }

    /// Generate random float 0.0 to 1.0 using the OS CSPRNG
    pub fn random_float(&self) -> f64 {
        use std::io::Read;
        let mut buf = [0u8; 8];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut buf))
            .expect("Failed to read from /dev/urandom");
        let raw: [u8; 8] = buf;
        (u64::from_le_bytes(raw) as f64) / (u64::MAX as f64)
    }

    /// Generate random integer in range [min, max] (inclusive), overflow-safe
    /// for the entire i64 domain. When `min > max`, returns `min` (matching
    /// the native backend's contract). Computes the inclusive span in unsigned
    /// arithmetic so a full-domain request cannot overflow.
    pub fn random_int(&self, min: i64, max: i64) -> i64 {
        if max <= min {
            return min;
        }
        let lo = min as u64;
        let hi = max as u64;
        // hi > lo, so `hi - lo` is in [1, UINT64_MAX]; adding one wraps to 0
        // exactly for the full 2^64-domain, in which case every value is valid.
        let span = hi.wrapping_sub(lo).wrapping_add(1);
        let mut raw = [0u8; 8];
        raw.copy_from_slice(&self.random_bytes(8));
        let bits = u64::from_le_bytes(raw);
        if span == 0 {
            lo.wrapping_add(bits) as i64
        } else {
            lo.wrapping_add(bits % span) as i64
        }
    }

    // ========================================================================
    // String Operations
    // ========================================================================

    /// Get character code at index (returns -1 if out of bounds)
    pub fn str_char_code(&self, s: &str, index: i64) -> i64 {
        if index < 0 {
            return -1;
        }
        s.chars()
            .nth(index as usize)
            .map(|c| c as i64)
            .unwrap_or(-1)
    }

    /// Create a string from a character code
    pub fn str_from_char_code(&self, code: i64) -> String {
        if !(0..=0x10FFFF).contains(&code) {
            return String::new();
        }
        char::from_u32(code as u32)
            .map(|c| c.to_string())
            .unwrap_or_default()
    }

    /// Convert string to uppercase
    pub fn str_to_upper(&self, s: &str) -> String {
        s.to_uppercase()
    }

    /// Convert string to lowercase
    pub fn str_to_lower(&self, s: &str) -> String {
        s.to_lowercase()
    }

    /// Get substring from start to end (exclusive)
    pub fn str_substring(&self, s: &str, start: i64, end: i64) -> String {
        let len = s.chars().count() as i64;
        let start = start.max(0).min(len) as usize;
        let end = end.max(0).min(len) as usize;
        if start >= end {
            return String::new();
        }
        s.chars().skip(start).take(end - start).collect()
    }

    /// Find first occurrence of substring (returns -1 if not found)
    pub fn str_index_of(&self, s: &str, substr: &str) -> i64 {
        if substr.is_empty() {
            return 0;
        }
        // Find byte position and convert to char position
        s.find(substr)
            .map(|byte_pos| s[..byte_pos].chars().count() as i64)
            .unwrap_or(-1)
    }

    /// Split string by delimiter
    pub fn str_split(&self, s: &str, delimiter: &str) -> Vec<String> {
        if delimiter.is_empty() {
            // Split into individual characters
            s.chars().map(|c| c.to_string()).collect()
        } else {
            s.split(delimiter).map(|s| s.to_string()).collect()
        }
    }

    /// Trim whitespace from both ends
    pub fn str_trim(&self, s: &str) -> String {
        s.trim().to_string()
    }

    /// Trim whitespace from start
    pub fn str_trim_start(&self, s: &str) -> String {
        s.trim_start().to_string()
    }

    /// Trim whitespace from end
    pub fn str_trim_end(&self, s: &str) -> String {
        s.trim_end().to_string()
    }

    // ========================================================================
    // Base64 Encoding/Decoding
    // ========================================================================

    /// Encode string to base64 (standard encoding)
    pub fn base64_encode(&self, input: &str) -> String {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        STANDARD.encode(input.as_bytes())
    }

    /// Decode base64 to string (standard encoding)
    pub fn base64_decode(&self, input: &str) -> Result<String, String> {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let bytes = STANDARD
            .decode(input)
            .map_err(|e| format!("Base64 decode error: {}", e))?;
        String::from_utf8(bytes).map_err(|e| format!("UTF-8 decode error: {}", e))
    }

    /// Encode string to base64 (URL-safe encoding)
    pub fn base64_encode_url(&self, input: &str) -> String {
        use base64::{engine::general_purpose::URL_SAFE, Engine as _};
        URL_SAFE.encode(input.as_bytes())
    }

    /// Decode URL-safe base64 to string
    pub fn base64_decode_url(&self, input: &str) -> Result<String, String> {
        use base64::{engine::general_purpose::URL_SAFE, Engine as _};
        let bytes = URL_SAFE
            .decode(input)
            .map_err(|e| format!("Base64 decode error: {}", e))?;
        String::from_utf8(bytes).map_err(|e| format!("UTF-8 decode error: {}", e))
    }

    // ========================================================================
    // Cryptographic Hash Functions
    // ========================================================================

    /// Compute MD5 hash of string, return hex
    pub fn hash_md5(&self, input: &str) -> String {
        let mut hasher = Md5::new();
        Digest::update(&mut hasher, input.as_bytes());
        hex::encode(hasher.finalize())
    }

    /// Compute SHA1 hash of string, return hex
    pub fn hash_sha1(&self, input: &str) -> String {
        let mut hasher = Sha1::new();
        Digest::update(&mut hasher, input.as_bytes());
        hex::encode(hasher.finalize())
    }

    /// Compute SHA256 hash of string, return hex
    pub fn hash_sha256(&self, input: &str) -> String {
        let mut hasher = Sha256::new();
        Digest::update(&mut hasher, input.as_bytes());
        hex::encode(hasher.finalize())
    }

    /// Compute SHA512 hash of string, return hex
    pub fn hash_sha512(&self, input: &str) -> String {
        let mut hasher = Sha512::new();
        Digest::update(&mut hasher, input.as_bytes());
        hex::encode(hasher.finalize())
    }

    // ========================================================================

    // ========================================================================
    // URL Encoding/Decoding
    // ========================================================================

    /// URL encode a string (percent-encoding)
    pub fn url_encode(&self, input: &str) -> String {
        let mut result = String::with_capacity(input.len() * 3);
        for byte in input.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    result.push(byte as char);
                }
                b' ' => result.push('+'),
                _ => {
                    result.push('%');
                    result.push_str(&format!("{:02X}", byte));
                }
            }
        }
        result
    }

    /// URL decode a string (percent-decoding)
    pub fn url_decode(&self, input: &str) -> String {
        fn hex_value(byte: u8) -> Option<u8> {
            match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            }
        }

        let bytes = input.as_bytes();
        let mut decoded = Vec::with_capacity(input.len());
        let mut index = 0;
        while index < bytes.len() {
            match bytes[index] {
                b'%' => {
                    let remaining = bytes.len() - index;
                    if remaining < 3 {
                        // Match the existing decoder contract: a malformed
                        // escape consumes the percent and any trailing digits.
                        break;
                    }
                    if let (Some(high), Some(low)) =
                        (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
                    {
                        decoded.push((high << 4) | low);
                    }
                    // Invalid hex escapes are consumed, but produce no byte.
                    index += 3;
                }
                b'+' => {
                    decoded.push(b' ');
                    index += 1;
                }
                byte => {
                    decoded.push(byte);
                    index += 1;
                }
            }
        }

        match String::from_utf8(decoded) {
            Ok(result) => result,
            Err(error) => {
                eprintln!("url_decode error: UTF-8 decode error: {}", error);
                String::new()
            }
        }
    }

    // ========================================================================
    // JSON Operations
    // ========================================================================

    /// Parse JSON string to Lira Value
    pub fn json_parse(&self, json_str: &str) -> Result<Value, String> {
        check_json_resources(json_str).map_err(|error| match error {
            JsonResourceError::Size => JSON_SIZE_LIMIT_ERROR.to_owned(),
            JsonResourceError::Resource => JSON_RESOURCE_LIMIT_ERROR.to_owned(),
        })?;
        let parsed: JsonValue =
            serde_json::from_str(json_str).map_err(|e| format!("JSON parse error: {}", e))?;
        let mut state = JsonBuildState { nodes: 0 };
        self.json_to_value(parsed, 0, &mut state)
    }

    fn json_to_value(
        &self,
        json: JsonValue,
        depth: usize,
        state: &mut JsonBuildState,
    ) -> Result<Value, String> {
        state.visit(depth)?;
        match json {
            JsonValue::Null => Ok(Value::Null),
            JsonValue::Bool(b) => Ok(Value::Bool(b)),
            JsonValue::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(Value::Int(i))
                } else {
                    n.as_f64()
                        .filter(|value| value.is_finite())
                        .map(Value::Float)
                        .ok_or_else(|| JSON_RESOURCE_LIMIT_ERROR.to_owned())
                }
            }
            JsonValue::String(s) => Ok(Value::String(Rc::new(s))),
            JsonValue::Array(arr) => {
                let values = arr
                    .into_iter()
                    .map(|value| self.json_to_value(value, depth + 1, state))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Value::Array(Gc::new(GcCell::new(values))))
            }
            JsonValue::Object(map) => {
                let mut obj = HashMap::new();
                for (k, v) in map {
                    obj.insert(k, self.json_to_value(v, depth + 1, state)?);
                }
                Ok(Value::Object(Gc::new(GcCell::new(obj))))
            }
        }
    }

    /// Stringify Lira Value to JSON
    pub fn json_stringify(&self, value: &Value) -> String {
        self.stringify_json(value, false)
    }

    /// Stringify with pretty printing
    pub fn json_stringify_pretty(&self, value: &Value) -> String {
        self.stringify_json(value, true)
    }

    fn stringify_json(&self, value: &Value, pretty: bool) -> String {
        let mut state = JsonConvertState {
            nodes: 0,
            active: HashSet::new(),
        };
        let Ok(json) = self.value_to_json(value, 0, &mut state) else {
            return "null".to_owned();
        };
        let output = if pretty {
            serde_json::to_string_pretty(&json)
        } else {
            serde_json::to_string(&json)
        };
        match output {
            Ok(output) if output.len() <= MAX_JSON_OUTPUT_BYTES => output,
            _ => "null".to_owned(),
        }
    }

    fn value_to_json(
        &self,
        value: &Value,
        depth: usize,
        state: &mut JsonConvertState,
    ) -> Result<JsonValue, JsonConvertError> {
        state.visit(depth)?;
        match value {
            Value::Null => Ok(JsonValue::Null),
            Value::Bool(b) => Ok(JsonValue::Bool(*b)),
            Value::Int(i) => Ok(JsonValue::Number((*i).into())),
            Value::Float(f) => Ok(serde_json::Number::from_f64(*f)
                .map(JsonValue::Number)
                .unwrap_or(JsonValue::Null)),
            Value::String(s) => Ok(JsonValue::String((**s).clone())),
            Value::Array(arr) => {
                let identity = Gc::as_ptr(arr) as usize;
                if !state.active.insert(identity) {
                    return Ok(JsonValue::Null);
                }
                let result = arr
                    .borrow()
                    .iter()
                    .map(|value| self.value_to_json(value, depth + 1, state))
                    .collect::<Result<Vec<_>, _>>();
                state.active.remove(&identity);
                Ok(JsonValue::Array(result?))
            }
            // JSON has no tuple type; preserve tuple element order as an
            // array while retaining the tuple representation inside the VM.
            Value::Tuple(tuple) => {
                let identity = Gc::as_ptr(tuple) as usize;
                if !state.active.insert(identity) {
                    return Ok(JsonValue::Null);
                }
                let result = tuple
                    .borrow()
                    .elements
                    .iter()
                    .map(|value| self.value_to_json(value, depth + 1, state))
                    .collect::<Result<Vec<_>, _>>();
                state.active.remove(&identity);
                Ok(JsonValue::Array(result?))
            }
            Value::Object(obj) | Value::Struct(obj) => {
                let identity = Gc::as_ptr(obj) as usize;
                if !state.active.insert(identity) {
                    return Ok(JsonValue::Null);
                }
                let mut entries: Vec<_> = obj
                    .borrow()
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                entries.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
                let result = entries
                    .into_iter()
                    .map(|(key, value)| {
                        self.value_to_json(&value, depth + 1, state)
                            .map(|value| (key, value))
                    })
                    .collect::<Result<Vec<_>, _>>();
                state.active.remove(&identity);
                let mut map = Map::new();
                for (key, value) in result? {
                    map.insert(key, value);
                }
                Ok(JsonValue::Object(map))
            }
            // Interfaces, functions, closures, fibers, and channels can't be
            // serialized to JSON without exposing opaque runtime state.
            Value::Interface(_) => Ok(JsonValue::Null),
            _ => Ok(JsonValue::Null),
        }
    }

    // ========================================================================
    // TCP Networking Operations
    // ========================================================================

    /// Connect (blocking) with no `&self` — runs on a pool thread. Returns the
    /// owned stream; the caller allocates the id and inserts it.
    pub(crate) fn tcp_connect_blocking(host: &str, port: i64) -> Option<TcpStream> {
        use std::net::ToSocketAddrs;
        let addr = format!("{}:{}", host, port);
        let socket_addr = addr.to_socket_addrs().ok()?.next()?;
        let stream =
            TcpStream::connect_timeout(&socket_addr, std::time::Duration::from_secs(2)).ok()?;
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(30)))
            .ok();
        stream
            .set_write_timeout(Some(std::time::Duration::from_secs(30)))
            .ok();
        Some(stream)
    }

    /// Write to an owned socket (checked out of the registry). Off-VM-thread safe.
    pub(crate) fn tcp_write_blocking(stream: &mut TcpStream, data: &str) -> i64 {
        match stream.write(data.as_bytes()) {
            Ok(n) => {
                stream.flush().ok();
                n as i64
            }
            Err(_) => -1,
        }
    }

    /// Read from an owned socket (checked out of the registry). Off-VM-thread safe.
    pub(crate) fn tcp_read_blocking(stream: &mut TcpStream, max_bytes: i64) -> String {
        let mut buffer = vec![0u8; max_bytes.clamp(0, 65536) as usize];
        match stream.read(&mut buffer) {
            Ok(n) => {
                buffer.truncate(n);
                String::from_utf8_lossy(&buffer).to_string()
            }
            Err(_) => String::new(),
        }
    }

    /// Connect to TCP server, returns socket id or -1 on error
    pub fn tcp_connect(&mut self, host: &str, port: i64) -> i64 {
        match Self::tcp_connect_blocking(host, port) {
            Some(stream) => {
                let id = self.next_socket_id;
                self.next_socket_id += 1;
                self.tcp_sockets.insert(id, stream);
                id
            }
            None => -1,
        }
    }

    /// Write data to socket, returns bytes written or -1
    pub fn tcp_write(&mut self, socket_id: i64, data: &str) -> i64 {
        match self.tcp_sockets.get_mut(&socket_id) {
            Some(stream) => Self::tcp_write_blocking(stream, data),
            None => -1,
        }
    }

    /// Read data from socket (up to max_bytes)
    pub fn tcp_read(&mut self, socket_id: i64, max_bytes: i64) -> String {
        match self.tcp_sockets.get_mut(&socket_id) {
            Some(stream) => Self::tcp_read_blocking(stream, max_bytes),
            None => String::new(),
        }
    }

    /// Read a line from socket
    pub fn tcp_read_line(&mut self, socket_id: i64) -> String {
        if let Some(stream) = self.tcp_sockets.get_mut(&socket_id) {
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(_) => line,
                Err(_) => String::new(),
            }
        } else {
            String::new()
        }
    }

    /// Close socket
    pub fn tcp_close(&mut self, socket_id: i64) -> bool {
        self.tcp_sockets.remove(&socket_id).is_some()
    }

    /// DNS lookup - resolve hostname to IP
    pub fn dns_lookup(&self, hostname: &str) -> String {
        Self::dns_lookup_blocking(hostname)
    }

    /// Blocking DNS resolution with no `&self` — safe to run off the VM thread.
    pub(crate) fn dns_lookup_blocking(hostname: &str) -> String {
        use std::net::ToSocketAddrs;
        let addr = format!("{}:80", hostname);
        match addr.to_socket_addrs() {
            Ok(mut addrs) => addrs.next().map(|a| a.ip().to_string()).unwrap_or_default(),
            Err(_) => String::new(),
        }
    }

    // ========================================================================
    // OS Operations
    // ========================================================================

    /// Get current working directory
    pub fn os_getcwd(&self) -> String {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    /// Change current directory
    pub fn os_chdir(&self, path: &str) -> bool {
        std::env::set_current_dir(path).is_ok()
    }

    /// Create directory
    pub fn os_mkdir(&self, path: &str) -> bool {
        std::fs::create_dir(path).is_ok()
    }

    /// Create directory and parents
    pub fn os_mkdir_all(&self, path: &str) -> bool {
        std::fs::create_dir_all(path).is_ok()
    }

    /// Remove empty directory
    pub fn os_rmdir(&self, path: &str) -> bool {
        std::fs::remove_dir(path).is_ok()
    }

    /// Remove file
    pub fn os_remove(&self, path: &str) -> bool {
        std::fs::remove_file(path).is_ok()
    }

    /// Remove directory and all contents
    pub fn os_remove_all(&self, path: &str) -> bool {
        std::fs::remove_dir_all(path).is_ok()
    }

    /// List directory contents
    pub fn os_listdir(&self, path: &str) -> Vec<String> {
        std::fs::read_dir(path)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Check if path is a directory
    pub fn os_is_dir(&self, path: &str) -> bool {
        std::path::Path::new(path).is_dir()
    }

    /// Check if path is a file
    pub fn os_is_file(&self, path: &str) -> bool {
        std::path::Path::new(path).is_file()
    }

    /// Rename/move file or directory
    pub fn os_rename(&self, from: &str, to: &str) -> bool {
        std::fs::rename(from, to).is_ok()
    }

    /// Copy file
    pub fn os_copy(&self, from: &str, to: &str) -> bool {
        std::fs::copy(from, to).is_ok()
    }

    // ========================================================================
    // Math Functions
    // ========================================================================

    /// Square root
    pub fn math_sqrt(&self, x: f64) -> f64 {
        x.sqrt()
    }

    /// Power function
    pub fn math_pow(&self, base: f64, exp: f64) -> f64 {
        base.powf(exp)
    }

    /// Exponential (e^x)
    pub fn math_exp(&self, x: f64) -> f64 {
        x.exp()
    }

    /// Natural logarithm
    pub fn math_ln(&self, x: f64) -> f64 {
        x.ln()
    }

    /// Base-10 logarithm
    pub fn math_log10(&self, x: f64) -> f64 {
        x.log10()
    }

    /// Base-2 logarithm
    pub fn math_log2(&self, x: f64) -> f64 {
        x.log2()
    }

    /// Sine
    pub fn math_sin(&self, x: f64) -> f64 {
        x.sin()
    }

    /// Cosine
    pub fn math_cos(&self, x: f64) -> f64 {
        x.cos()
    }

    /// Tangent
    pub fn math_tan(&self, x: f64) -> f64 {
        x.tan()
    }

    /// Arcsine
    pub fn math_asin(&self, x: f64) -> f64 {
        x.asin()
    }

    /// Arccosine
    pub fn math_acos(&self, x: f64) -> f64 {
        x.acos()
    }

    /// Arctangent
    pub fn math_atan(&self, x: f64) -> f64 {
        x.atan()
    }

    /// Two-argument arctangent
    pub fn math_atan2(&self, y: f64, x: f64) -> f64 {
        y.atan2(x)
    }

    /// Hyperbolic sine
    pub fn math_sinh(&self, x: f64) -> f64 {
        x.sinh()
    }

    /// Hyperbolic cosine
    pub fn math_cosh(&self, x: f64) -> f64 {
        x.cosh()
    }

    /// Hyperbolic tangent
    pub fn math_tanh(&self, x: f64) -> f64 {
        x.tanh()
    }

    /// Floor (round down)
    pub fn math_floor(&self, x: f64) -> f64 {
        x.floor()
    }

    /// Ceiling (round up)
    pub fn math_ceil(&self, x: f64) -> f64 {
        x.ceil()
    }

    /// Round to nearest integer
    pub fn math_round(&self, x: f64) -> f64 {
        x.round()
    }

    /// Truncate (round towards zero)
    pub fn math_trunc(&self, x: f64) -> f64 {
        x.trunc()
    }

    /// Absolute value for floats
    pub fn math_abs_float(&self, x: f64) -> f64 {
        x.abs()
    }

    /// Check if value is NaN
    pub fn math_is_nan(&self, x: f64) -> bool {
        x.is_nan()
    }

    /// Check if value is infinite
    pub fn math_is_infinite(&self, x: f64) -> bool {
        x.is_infinite()
    }

    /// Check if value is finite
    pub fn math_is_finite(&self, x: f64) -> bool {
        x.is_finite()
    }

    // ========================================================================
    // HTTP Client Operations
    // ========================================================================

    const HTTP_GLOBAL_TIMEOUT: Duration = Duration::from_secs(30);
    const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
    const HTTP_MAX_RESPONSE_BODY_BYTES: u64 = 10 * 1024 * 1024;

    /// A blocking HTTP agent configured so non-2xx responses come back as `Ok`
    /// (the builtins surface the status code + body to the caller rather than
    /// treating 4xx/5xx as transport errors).
    fn http_agent() -> ureq::Agent {
        ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .http_status_as_error(false)
                .timeout_global(Some(Self::HTTP_GLOBAL_TIMEOUT))
                .timeout_resolve(Some(Self::HTTP_CONNECT_TIMEOUT))
                .timeout_connect(Some(Self::HTTP_CONNECT_TIMEOUT))
                .timeout_send_request(Some(Self::HTTP_GLOBAL_TIMEOUT))
                .timeout_send_body(Some(Self::HTTP_GLOBAL_TIMEOUT))
                .timeout_recv_response(Some(Self::HTTP_GLOBAL_TIMEOUT))
                .timeout_recv_body(Some(Self::HTTP_GLOBAL_TIMEOUT))
                .build(),
        )
    }

    /// Read a response body with the same bounded, text-oriented behavior as
    /// the native runtime. Once a response status exists, body failures are
    /// represented by an empty body by the callers below.
    fn read_http_body(response: &mut ureq::http::Response<ureq::Body>) -> String {
        response
            .body_mut()
            .with_config()
            .limit(Self::HTTP_MAX_RESPONSE_BODY_BYTES)
            .lossy_utf8(true)
            .read_to_string()
            .unwrap_or_default()
    }

    /// Render response headers as `Name: value` lines.
    fn format_headers(headers: &ureq::http::HeaderMap) -> String {
        headers
            .iter()
            .map(|(name, value)| format!("{}: {}", name, value.to_str().unwrap_or("")))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// HTTP GET request. Returns (status_code, headers, body) or error.
    pub fn http_get(&self, url: &str) -> Result<(i64, String, String), String> {
        Self::http_get_blocking(url)
    }

    /// Blocking HTTP GET with no `&self` — safe to run on the I/O thread pool
    /// (it only touches owned data and builds its own agent).
    pub(crate) fn http_get_blocking(url: &str) -> Result<(i64, String, String), String> {
        let req = ureq::http::Request::get(url)
            .body(())
            .map_err(|e| format!("Invalid request: {}", e))?;
        let mut resp = Self::http_agent()
            .run(req)
            .map_err(|e| format!("HTTP error: {}", e))?;
        let status = resp.status().as_u16() as i64;
        let headers = Self::format_headers(resp.headers());
        let body = Self::read_http_body(&mut resp);
        Ok((status, headers, body))
    }

    /// HTTP POST request. Returns (status_code, headers, body) or error.
    pub fn http_post(
        &self,
        url: &str,
        body: &str,
        content_type: &str,
    ) -> Result<(i64, String, String), String> {
        Self::http_post_blocking(url, body, content_type)
    }

    /// Blocking HTTP POST with no `&self` — safe to run off the VM thread.
    pub(crate) fn http_post_blocking(
        url: &str,
        body: &str,
        content_type: &str,
    ) -> Result<(i64, String, String), String> {
        let req = ureq::http::Request::post(url)
            .header("Content-Type", content_type)
            .body(body.to_string())
            .map_err(|e| format!("Invalid request: {}", e))?;
        let mut resp = Self::http_agent()
            .run(req)
            .map_err(|e| format!("HTTP error: {}", e))?;
        let status = resp.status().as_u16() as i64;
        let headers = Self::format_headers(resp.headers());
        let resp_body = Self::read_http_body(&mut resp);
        Ok((status, headers, resp_body))
    }

    /// HTTP request with custom method and headers. Returns (status_code, body).
    pub fn http_request(
        &self,
        method: &str,
        url: &str,
        headers_str: &str,
        body: &str,
    ) -> Result<(i64, String), String> {
        Self::http_request_blocking(method, url, headers_str, body)
    }

    /// Blocking custom HTTP request with no `&self` — safe to run off the VM thread.
    pub(crate) fn http_request_blocking(
        method: &str,
        url: &str,
        headers_str: &str,
        body: &str,
    ) -> Result<(i64, String), String> {
        let mut builder = ureq::http::Request::builder().method(method).uri(url);
        for line in headers_str.lines() {
            if let Some((name, value)) = line.split_once(':') {
                let name = name.trim();
                let value = value.trim();
                if let (Ok(name), Ok(value)) = (
                    name.parse::<ureq::http::HeaderName>(),
                    value.parse::<ureq::http::HeaderValue>(),
                ) {
                    builder = builder.header(name, value);
                }
            }
        }
        let req = builder
            .body(body.to_string())
            .map_err(|e| format!("Invalid request: {}", e))?;
        let mut resp = Self::http_agent()
            .run(req)
            .map_err(|e| format!("HTTP error: {}", e))?;
        let status = resp.status().as_u16() as i64;
        let resp_body = Self::read_http_body(&mut resp);
        Ok((status, resp_body))
    }

    // ========================================================================
    // Regex Operations
    // ========================================================================

    /// Check if pattern matches string
    pub fn regex_match(&self, pattern: &str, text: &str) -> bool {
        valid_regex_input(pattern, text)
            && build_regex(pattern).is_some_and(|regex| regex.is_match(text))
    }

    /// Find first match, return matched string or empty
    pub fn regex_find(&self, pattern: &str, text: &str) -> String {
        if !valid_regex_input(pattern, text) {
            return String::new();
        }
        build_regex(pattern)
            .and_then(|regex| regex.find(text).map(|m| m.as_str().to_owned()))
            .unwrap_or_default()
    }

    /// Find all matches, return array of strings
    pub fn regex_find_all(&self, pattern: &str, text: &str) -> Vec<String> {
        if !valid_regex_input(pattern, text) {
            return Vec::new();
        }
        let Some(regex) = build_regex(pattern) else {
            return Vec::new();
        };
        let count = regex
            .find_iter(text)
            .take(MAX_REGEX_RESULT_COUNT + 1)
            .count();
        if count > MAX_REGEX_RESULT_COUNT {
            return Vec::new();
        }
        regex
            .find_iter(text)
            .map(|m| m.as_str().to_owned())
            .collect()
    }

    /// Replace first occurrence
    pub fn regex_replace(&self, pattern: &str, text: &str, replacement: &str) -> String {
        if !valid_regex_input(pattern, text) || replacement.len() > MAX_REGEX_INPUT_BYTES {
            return text.to_owned();
        }
        build_regex(pattern)
            .and_then(|regex| bounded_regex_replace(&regex, text, replacement, false))
            .unwrap_or_else(|| text.to_owned())
    }

    /// Replace all occurrences
    pub fn regex_replace_all(&self, pattern: &str, text: &str, replacement: &str) -> String {
        if !valid_regex_input(pattern, text) || replacement.len() > MAX_REGEX_INPUT_BYTES {
            return text.to_owned();
        }
        build_regex(pattern)
            .and_then(|regex| bounded_regex_replace(&regex, text, replacement, true))
            .unwrap_or_else(|| text.to_owned())
    }

    /// Split by pattern
    pub fn regex_split(&self, pattern: &str, text: &str) -> Vec<String> {
        if !valid_regex_input(pattern, text) {
            return vec![text.to_owned()];
        }
        let Some(regex) = build_regex(pattern) else {
            return vec![text.to_owned()];
        };
        let count = regex.split(text).take(MAX_REGEX_RESULT_COUNT + 1).count();
        if count > MAX_REGEX_RESULT_COUNT {
            return vec![text.to_owned()];
        }
        regex.split(text).map(str::to_owned).collect()
    }

    /// Get capture groups from first match
    pub fn regex_captures(&self, pattern: &str, text: &str) -> Vec<String> {
        if !valid_regex_input(pattern, text) {
            return Vec::new();
        }
        build_regex(pattern)
            .and_then(|regex| regex.captures(text))
            .map(|caps| {
                let mut total_bytes = 0usize;
                let mut values = Vec::new();
                for capture in caps.iter().flatten() {
                    let Some(next_bytes) = total_bytes.checked_add(capture.as_str().len()) else {
                        return Vec::new();
                    };
                    if next_bytes > MAX_REGEX_OUTPUT_BYTES || values.len() == MAX_REGEX_RESULT_COUNT
                    {
                        return Vec::new();
                    }
                    total_bytes = next_bytes;
                    values.push(capture.as_str().to_owned());
                }
                values
            })
            .unwrap_or_default()
    }

    /// Check if pattern is valid regex
    pub fn regex_is_valid(&self, pattern: &str) -> bool {
        build_regex(pattern).is_some()
    }

    // ========================================================================
    // UUID Operations
    // ========================================================================

    /// Generate UUID v4 (random)
    pub fn uuid_v4(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }

    /// Generate UUID v7 (time-ordered)
    pub fn uuid_v7(&self) -> String {
        uuid::Uuid::now_v7().to_string()
    }

    /// Parse and validate UUID string
    pub fn uuid_is_valid(&self, s: &str) -> bool {
        uuid::Uuid::parse_str(s).is_ok()
    }

    /// Get nil UUID
    pub fn uuid_nil(&self) -> String {
        uuid::Uuid::nil().to_string()
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod json_tests {
    use super::*;

    #[test]
    fn malformed_json_reports_parse_error() {
        let error = Runtime::new().json_parse("{").unwrap_err();
        assert!(
            error.starts_with("JSON parse error:"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn nested_json_round_trips() {
        let runtime = Runtime::new();
        let value = runtime
            .json_parse(r#"{"outer":{"items":[1,true,"ok"]}}"#)
            .expect("valid JSON");
        assert_eq!(
            runtime.json_stringify(&value),
            r#"{"outer":{"items":[1,true,"ok"]}}"#
        );
    }

    #[test]
    fn resource_preflight_rejects_depth_and_node_boundaries() {
        let too_deep = format!(
            "{}0{}",
            "[".repeat(MAX_JSON_DEPTH + 1),
            "]".repeat(MAX_JSON_DEPTH + 1)
        );
        assert_eq!(
            check_json_resources(&too_deep),
            Err(JsonResourceError::Resource)
        );
        assert!(matches!(
            Runtime::new().json_parse(&too_deep),
            Err(error) if error == JSON_RESOURCE_LIMIT_ERROR
        ));

        let too_many_nodes = format!(
            "[{}]",
            (0..=MAX_JSON_NODES).map(|_| "0,").collect::<String>()
        );
        assert_eq!(
            check_json_resources(&too_many_nodes),
            Err(JsonResourceError::Resource)
        );
    }

    #[test]
    fn stringify_cycles_unsupported_and_nonfinite_values_as_null() {
        let runtime = Runtime::new();
        let array = Gc::new(GcCell::new(Vec::new()));
        array.borrow_mut().push(Value::Array(array.clone()));
        assert_eq!(runtime.json_stringify(&Value::Array(array)), "[null]");
        assert_eq!(runtime.json_stringify(&Value::Function(1)), "null");
        assert_eq!(runtime.json_stringify(&Value::Float(f64::NAN)), "null");
    }

    #[test]
    fn stringify_checks_output_size() {
        let value = Value::String(Rc::new("x".repeat(MAX_JSON_OUTPUT_BYTES + 1)));
        assert_eq!(Runtime::new().json_stringify(&value), "null");
    }
}

#[cfg(test)]
mod env_tests {
    use super::Runtime;

    #[test]
    fn per_runtime_override_wins_without_mutating_process_environment() {
        const NAME: &str = "LIRA_VM_ENV_OVERRIDE_TEST_UNIQUE";
        let process_value = std::env::var(NAME).ok();
        let mut runtime = Runtime::new();
        runtime.set_env_override(NAME, "local-value");

        assert_eq!(runtime.env_get(NAME).as_deref(), Some("local-value"));
        assert_eq!(std::env::var(NAME).ok(), process_value);
    }
}

#[cfg(test)]
mod url_decode_tests {
    use super::Runtime;

    #[test]
    fn percent_decoding_preserves_utf8_and_plus_behavior() {
        let runtime = Runtime::new();
        assert_eq!(runtime.url_decode("%C3%A9"), "é");
        assert_eq!(runtime.url_decode("hello+world"), "hello world");
        assert_eq!(runtime.url_decode("%E2%9C%93+ok"), "✓ ok");
    }

    #[test]
    fn invalid_utf8_returns_empty_without_panicking() {
        let runtime = Runtime::new();
        assert_eq!(runtime.url_decode("%FF"), "");
        assert_eq!(runtime.url_decode("%C3%28"), "");
    }

    #[test]
    fn malformed_percent_escapes_are_bounded() {
        let runtime = Runtime::new();
        assert_eq!(runtime.url_decode("%"), "");
        assert_eq!(runtime.url_decode("%A"), "");
        assert_eq!(runtime.url_decode("%GGtail"), "tail");
    }
}

#[cfg(test)]
mod regex_tests {
    use super::*;

    #[test]
    fn ordinary_unicode_and_invalid_patterns_keep_existing_behavior() {
        let runtime = Runtime::new();
        assert!(runtime.regex_match(r"\p{Greek}+", "γειά"));
        assert_eq!(runtime.regex_find_all(r"[,;]", "a,b;c"), [",", ";"]);
        assert_eq!(runtime.regex_split(r"[,;]", "a,b;c"), ["a", "b", "c"]);
        assert_eq!(runtime.regex_captures(r"(a)(b)?", "a"), ["a", "a"]);
        assert_eq!(
            runtime.regex_replace_all(r"(?P<word>[a-z]+)-([0-9]+)", "abc-42", "${word}:$2"),
            "abc:42"
        );
        assert_eq!(
            runtime.regex_replace_all(r"[0-9]+", "a1b22", "[$0]/$$"),
            "a[1]/$b[22]/$"
        );
        assert!(!runtime.regex_is_valid("["));
        assert_eq!(runtime.regex_replace_all("[", "abc", "x"), "abc");
        assert_eq!(runtime.regex_split("[", "abc"), ["abc"]);
    }

    #[test]
    fn oversized_pattern_and_input_use_deterministic_fallbacks() {
        let runtime = Runtime::new();
        let pattern = "a".repeat(MAX_REGEX_PATTERN_BYTES + 1);
        let input = "a".repeat(MAX_REGEX_INPUT_BYTES + 1);
        assert!(!runtime.regex_is_valid(&pattern));
        assert!(!runtime.regex_match("a", &input));
        assert_eq!(runtime.regex_find("a", &input), "");
        assert!(runtime.regex_find_all("a", &input).is_empty());
        assert_eq!(runtime.regex_replace("a", &input, "x"), input);
        assert_eq!(runtime.regex_replace_all("a", &input, "x"), input);
        assert_eq!(runtime.regex_split("a", &input), [input.as_str()]);
        assert!(runtime.regex_captures("a", &input).is_empty());
    }

    #[test]
    fn result_count_and_output_limits_do_not_truncate() {
        let runtime = Runtime::new();
        let many = "a".repeat(MAX_REGEX_RESULT_COUNT + 1);
        assert!(runtime.regex_find_all("a", &many).is_empty());
        assert_eq!(runtime.regex_split("a", &many), [many.as_str()]);

        let input = "a".repeat(4 * 1024 * 1024);
        let replacement = "x".repeat(9);
        assert_eq!(runtime.regex_replace_all("a", &input, &replacement), input);
    }

    #[test]
    fn zero_width_matches_remain_bounded_and_preserve_unicode() {
        let runtime = Runtime::new();
        assert_eq!(runtime.regex_find_all("^|$", "é"), ["", ""]);
        assert_eq!(runtime.regex_split("^|$", "é"), ["", "é", ""]);
    }
}
