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
use md5::{Digest, Md5};
use regex::Regex;
use serde_json::{Map, Value as JsonValue};
use sha1::Sha1;
use sha2::{Sha256, Sha512};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::TcpStream;
use std::rc::Rc;

/// File handle for open files
pub type FileHandle = i64;

/// Runtime context for built-in function calls
pub struct Runtime {
    /// Standard output buffer
    stdout: String,
    /// Open file handles (fd -> File)
    files: HashMap<FileHandle, File>,
    /// Next file descriptor to assign
    next_fd: FileHandle,
    /// Open TCP sockets (socket_id -> TcpStream)
    tcp_sockets: HashMap<i64, TcpStream>,
    /// Next socket ID to assign
    next_socket_id: i64,
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            stdout: String::new(),
            files: HashMap::new(),
            next_fd: 10, // Start at 10 to avoid confusion with stdin/stdout/stderr
            tcp_sockets: HashMap::new(),
            next_socket_id: 100, // Start at 100 to distinguish from file handles
        }
    }

    /// Print a value to stdout
    pub fn print(&mut self, value: &Value) {
        print!("{}", value.to_string());
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
        Utc.with_ymd_and_hms(
            year as i32,
            month as u32,
            day as u32,
            hour as u32,
            min as u32,
            sec as u32,
        )
        .single()
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(0)
    }

    // ========================================================================
    // File I/O Operations
    // ========================================================================

    /// Open a file and return a handle
    /// mode: 0 = read, 1 = write, 2 = append, 3 = read+write
    pub fn file_open(&mut self, path: &str, mode: i64) -> Result<FileHandle, String> {
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

        let file = file.map_err(|e| format!("Failed to open '{}': {}", path, e))?;
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

        let mut buffer = vec![0u8; max_bytes.min(1024 * 1024) as usize]; // Cap at 1MB
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
        std::env::var(name).ok()
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

    /// Generate random bytes
    pub fn random_bytes(&self, count: usize) -> Vec<u8> {
        use std::collections::hash_map::RandomState;
        use std::hash::{BuildHasher, Hasher};
        // Use RandomState for simple random generation
        let mut bytes = Vec::with_capacity(count);
        for _ in 0..count {
            let s = RandomState::new();
            let mut hasher = s.build_hasher();
            hasher.write_u64(self.current_time_millis() as u64);
            bytes.push((hasher.finish() & 0xFF) as u8);
        }
        bytes
    }

    /// Generate random float 0.0 to 1.0
    pub fn random_float(&self) -> f64 {
        use std::collections::hash_map::RandomState;
        use std::hash::{BuildHasher, Hasher};
        let s = RandomState::new();
        let mut hasher = s.build_hasher();
        hasher.write_u64(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64,
        );
        (hasher.finish() as f64) / (u64::MAX as f64)
    }

    /// Generate random integer in range [min, max]
    pub fn random_int(&self, min: i64, max: i64) -> i64 {
        let range = (max - min + 1) as f64;
        min + (self.random_float() * range) as i64
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
        if code < 0 || code > 0x10FFFF {
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
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        STANDARD.encode(input.as_bytes())
    }

    /// Decode base64 to string (standard encoding)
    pub fn base64_decode(&self, input: &str) -> Result<String, String> {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        let bytes = STANDARD
            .decode(input)
            .map_err(|e| format!("Base64 decode error: {}", e))?;
        String::from_utf8(bytes).map_err(|e| format!("UTF-8 decode error: {}", e))
    }

    /// Encode string to base64 (URL-safe encoding)
    pub fn base64_encode_url(&self, input: &str) -> String {
        use base64::{Engine as _, engine::general_purpose::URL_SAFE};
        URL_SAFE.encode(input.as_bytes())
    }

    /// Decode URL-safe base64 to string
    pub fn base64_decode_url(&self, input: &str) -> Result<String, String> {
        use base64::{Engine as _, engine::general_purpose::URL_SAFE};
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
        let mut result = String::new();
        let mut chars = input.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '%' => {
                    let hex: String = chars.by_ref().take(2).collect();
                    if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                        result.push(byte as char);
                    }
                }
                '+' => result.push(' '),
                _ => result.push(c),
            }
        }
        result
    }

    // ========================================================================
    // JSON Operations
    // ========================================================================

    /// Parse JSON string to Lira Value
    pub fn json_parse(&self, json_str: &str) -> Result<Value, String> {
        let parsed: JsonValue =
            serde_json::from_str(json_str).map_err(|e| format!("JSON parse error: {}", e))?;
        Ok(self.json_to_value(parsed))
    }

    fn json_to_value(&self, json: JsonValue) -> Value {
        match json {
            JsonValue::Null => Value::Null,
            JsonValue::Bool(b) => Value::Bool(b),
            JsonValue::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::Int(i)
                } else {
                    Value::Float(n.as_f64().unwrap_or(0.0))
                }
            }
            JsonValue::String(s) => Value::String(Rc::new(s)),
            JsonValue::Array(arr) => {
                let values: Vec<Value> = arr.into_iter().map(|v| self.json_to_value(v)).collect();
                Value::Array(Rc::new(RefCell::new(values)))
            }
            JsonValue::Object(map) => {
                let mut obj = HashMap::new();
                for (k, v) in map {
                    obj.insert(k, self.json_to_value(v));
                }
                Value::Object(Rc::new(RefCell::new(obj)))
            }
        }
    }

    /// Stringify Lira Value to JSON
    pub fn json_stringify(&self, value: &Value) -> String {
        self.value_to_json(value).to_string()
    }

    /// Stringify with pretty printing
    pub fn json_stringify_pretty(&self, value: &Value) -> String {
        serde_json::to_string_pretty(&self.value_to_json(value)).unwrap_or_default()
    }

    fn value_to_json(&self, value: &Value) -> JsonValue {
        match value {
            Value::Null => JsonValue::Null,
            Value::Bool(b) => JsonValue::Bool(*b),
            Value::Int(i) => JsonValue::Number((*i).into()),
            Value::Float(f) => serde_json::Number::from_f64(*f)
                .map(JsonValue::Number)
                .unwrap_or(JsonValue::Null),
            Value::String(s) => JsonValue::String((**s).clone()),
            Value::Array(arr) => {
                let values: Vec<JsonValue> =
                    arr.borrow().iter().map(|v| self.value_to_json(v)).collect();
                JsonValue::Array(values)
            }
            Value::Object(obj) => {
                let mut map = Map::new();
                for (k, v) in obj.borrow().iter() {
                    map.insert(k.clone(), self.value_to_json(v));
                }
                JsonValue::Object(map)
            }
            // Functions, closures, fibers, channels can't be serialized to JSON
            _ => JsonValue::Null,
        }
    }

    // ========================================================================
    // TCP Networking Operations
    // ========================================================================

    /// Connect to TCP server, returns socket id or -1 on error
    pub fn tcp_connect(&mut self, host: &str, port: i64) -> i64 {
        use std::net::ToSocketAddrs;
        let addr = format!("{}:{}", host, port);
        // Resolve address first
        let socket_addr = match addr.to_socket_addrs() {
            Ok(mut addrs) => match addrs.next() {
                Some(a) => a,
                None => return -1,
            },
            Err(_) => return -1,
        };
        // Try to connect with timeout (2 seconds default)
        match TcpStream::connect_timeout(&socket_addr, std::time::Duration::from_secs(2)) {
            Ok(stream) => {
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(30)))
                    .ok();
                stream
                    .set_write_timeout(Some(std::time::Duration::from_secs(30)))
                    .ok();
                let id = self.next_socket_id;
                self.next_socket_id += 1;
                self.tcp_sockets.insert(id, stream);
                id
            }
            Err(_) => -1,
        }
    }

    /// Write data to socket, returns bytes written or -1
    pub fn tcp_write(&mut self, socket_id: i64, data: &str) -> i64 {
        if let Some(stream) = self.tcp_sockets.get_mut(&socket_id) {
            match stream.write(data.as_bytes()) {
                Ok(n) => {
                    stream.flush().ok();
                    n as i64
                }
                Err(_) => -1,
            }
        } else {
            -1
        }
    }

    /// Read data from socket (up to max_bytes)
    pub fn tcp_read(&mut self, socket_id: i64, max_bytes: i64) -> String {
        if let Some(stream) = self.tcp_sockets.get_mut(&socket_id) {
            let mut buffer = vec![0u8; max_bytes.min(65536) as usize];
            match stream.read(&mut buffer) {
                Ok(n) => {
                    buffer.truncate(n);
                    String::from_utf8_lossy(&buffer).to_string()
                }
                Err(_) => String::new(),
            }
        } else {
            String::new()
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
        use std::net::ToSocketAddrs;
        let addr = format!("{}:80", hostname);
        match addr.to_socket_addrs() {
            Ok(mut addrs) => {
                if let Some(addr) = addrs.next() {
                    addr.ip().to_string()
                } else {
                    String::new()
                }
            }
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

    /// HTTP GET request
    /// Returns (status_code, headers, body) or error
    pub fn http_get(&self, url: &str) -> Result<(i64, String, String), String> {
        match ureq::get(url).call() {
            Ok(response) => {
                let status = response.status() as i64;
                let headers = response
                    .headers_names()
                    .iter()
                    .map(|name| format!("{}: {}", name, response.header(name).unwrap_or("")))
                    .collect::<Vec<_>>()
                    .join("\n");
                let body = response.into_string().unwrap_or_default();
                Ok((status, headers, body))
            }
            Err(ureq::Error::Status(code, response)) => {
                let body = response.into_string().unwrap_or_default();
                Ok((code as i64, String::new(), body))
            }
            Err(e) => Err(format!("HTTP error: {}", e)),
        }
    }

    /// HTTP POST request
    /// Returns (status_code, headers, body) or error
    pub fn http_post(
        &self,
        url: &str,
        body: &str,
        content_type: &str,
    ) -> Result<(i64, String, String), String> {
        match ureq::post(url)
            .set("Content-Type", content_type)
            .send_string(body)
        {
            Ok(response) => {
                let status = response.status() as i64;
                let headers = String::new();
                let body = response.into_string().unwrap_or_default();
                Ok((status, headers, body))
            }
            Err(ureq::Error::Status(code, response)) => {
                let body = response.into_string().unwrap_or_default();
                Ok((code as i64, String::new(), body))
            }
            Err(e) => Err(format!("HTTP error: {}", e)),
        }
    }

    /// HTTP request with custom method and headers
    /// Returns (status_code, body) or error
    pub fn http_request(
        &self,
        method: &str,
        url: &str,
        headers_str: &str,
        body: &str,
    ) -> Result<(i64, String), String> {
        let mut request = match method.to_uppercase().as_str() {
            "GET" => ureq::get(url),
            "POST" => ureq::post(url),
            "PUT" => ureq::put(url),
            "DELETE" => ureq::delete(url),
            "PATCH" => ureq::patch(url),
            "HEAD" => ureq::head(url),
            _ => return Err(format!("Unsupported method: {}", method)),
        };

        // Parse and add headers
        for line in headers_str.lines() {
            if let Some((name, value)) = line.split_once(':') {
                request = request.set(name.trim(), value.trim());
            }
        }

        let response = if body.is_empty() {
            request.call()
        } else {
            request.send_string(body)
        };

        match response {
            Ok(resp) => {
                let status = resp.status() as i64;
                let body = resp.into_string().unwrap_or_default();
                Ok((status, body))
            }
            Err(ureq::Error::Status(code, resp)) => {
                let body = resp.into_string().unwrap_or_default();
                Ok((code as i64, body))
            }
            Err(e) => Err(format!("HTTP error: {}", e)),
        }
    }

    // ========================================================================
    // Regex Operations
    // ========================================================================

    /// Check if pattern matches string
    pub fn regex_match(&self, pattern: &str, text: &str) -> bool {
        Regex::new(pattern)
            .map(|re| re.is_match(text))
            .unwrap_or(false)
    }

    /// Find first match, return matched string or empty
    pub fn regex_find(&self, pattern: &str, text: &str) -> String {
        Regex::new(pattern)
            .ok()
            .and_then(|re| re.find(text))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default()
    }

    /// Find all matches, return array of strings
    pub fn regex_find_all(&self, pattern: &str, text: &str) -> Vec<String> {
        Regex::new(pattern)
            .map(|re| re.find_iter(text).map(|m| m.as_str().to_string()).collect())
            .unwrap_or_default()
    }

    /// Replace first occurrence
    pub fn regex_replace(&self, pattern: &str, text: &str, replacement: &str) -> String {
        Regex::new(pattern)
            .map(|re| re.replace(text, replacement).to_string())
            .unwrap_or_else(|_| text.to_string())
    }

    /// Replace all occurrences
    pub fn regex_replace_all(&self, pattern: &str, text: &str, replacement: &str) -> String {
        Regex::new(pattern)
            .map(|re| re.replace_all(text, replacement).to_string())
            .unwrap_or_else(|_| text.to_string())
    }

    /// Split by pattern
    pub fn regex_split(&self, pattern: &str, text: &str) -> Vec<String> {
        Regex::new(pattern)
            .map(|re| re.split(text).map(|s| s.to_string()).collect())
            .unwrap_or_else(|_| vec![text.to_string()])
    }

    /// Get capture groups from first match
    pub fn regex_captures(&self, pattern: &str, text: &str) -> Vec<String> {
        Regex::new(pattern)
            .ok()
            .and_then(|re| re.captures(text))
            .map(|caps| {
                caps.iter()
                    .filter_map(|m| m.map(|m| m.as_str().to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Check if pattern is valid regex
    pub fn regex_is_valid(&self, pattern: &str) -> bool {
        Regex::new(pattern).is_ok()
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

