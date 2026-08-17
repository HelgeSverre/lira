//! `serde_json` implementation of the native JSON ABI.
//!
//! The native backend represents dynamic values as pointers to `LiraAny`
//! objects.  This module deliberately uses the runtime's constructors for
//! every returned object; Rust owns no allocation which is visible to the
//! generated program.  It is kept separate from `lib.rs` so the regex and
//! JSON entry points can be enabled independently while the native runtime
//! ABI is still settling.

pub use crate::{LiraAny, LiraArray, LiraStr};
use serde_json::{Map, Number, Value};
use std::collections::HashSet;
use std::ffi::{c_char, c_void};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::slice;
use std::str;

const LIRA_KIND_STRING: u32 = 1;
const LIRA_KIND_ANY: u32 = 7;
const LIRA_ANY_NULL: i64 = 0;
const LIRA_ANY_BOOL: i64 = 1;
const LIRA_ANY_INT: i64 = 2;
const LIRA_ANY_FLOAT: i64 = 3;
const LIRA_ANY_STRING: i64 = 4;
const LIRA_ANY_ARRAY: i64 = 5;
const LIRA_ANY_OBJECT: i64 = 6;
const LIRA_ANY_REF: i64 = 7;
const LIRA_ANY_FUNCTION: i64 = 8;
const LIRA_ANY_CHANNEL: i64 = 9;
const LIRA_ANY_FIBER: i64 = 10;

// These limits are part of the native boundary rather than serde_json's
// parser configuration.  They make malformed or adversarial input fail in a
// bounded, deterministic way and cap recursive conversion before the C stack
// can be exhausted.
const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
const MAX_JSON_NODES: usize = 1_000_000;
const MAX_JSON_DEPTH: usize = 128;
const MAX_JSON_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
const MAX_ABI_BYTES: i64 = isize::MAX as i64 - 24;
const JSON_SIZE_LIMIT_ERROR: &str = "input exceeds JSON size limit";
const JSON_RESOURCE_LIMIT_ERROR: &str = "input exceeds JSON resource limit";

extern "C" {
    fn lira_rt_panic(message: *const c_char);
    fn lira_rt_str_new(bytes: *const c_char, len: i64) -> *mut LiraStr;
    fn lira_rt_array_new(capacity: i64) -> *mut LiraArray;
    fn lira_rt_array_push(array: *mut LiraArray, value: i64);
    fn lira_rt_array_len(array: *const LiraArray) -> i64;
    fn lira_rt_array_get(array: *const LiraArray, index: i64) -> i64;
    fn lira_rt_map_new() -> *mut c_void;
    fn lira_rt_map_set(map: *mut c_void, key: *mut LiraStr, value: i64);
    fn lira_rt_map_len(map: *mut c_void) -> i64;
    fn lira_rt_map_keys(map: *mut c_void) -> *mut LiraArray;

    fn lira_rt_any_null() -> *mut LiraAny;
    fn lira_rt_any_box_bool(value: i8) -> *mut LiraAny;
    fn lira_rt_any_box_int(value: i64) -> *mut LiraAny;
    fn lira_rt_any_box_float(value: f64) -> *mut LiraAny;
    fn lira_rt_any_box_string(value: *mut LiraStr) -> *mut LiraAny;
    fn lira_rt_any_box_array(value: *mut LiraArray) -> *mut LiraAny;
    fn lira_rt_any_box_map(value: *mut c_void) -> *mut LiraAny;
    fn lira_rt_any_array_at(value: *const LiraAny, index: i64) -> *mut LiraAny;
    fn lira_rt_any_object_at(value: *const LiraAny, key: *const LiraStr) -> *mut LiraAny;
}

/// Borrow an ABI string after validating its header, length, and UTF-8.
///
/// # Safety
///
/// The pointer must refer to a live native `LiraStr` allocation.  Generated
/// code satisfies that contract; this check additionally rejects malformed
/// headers and lengths before constructing the Rust slice.
unsafe fn read_str<'a>(value: *const LiraStr) -> Option<&'a str> {
    if value.is_null() || !(value as usize).is_multiple_of(std::mem::align_of::<LiraStr>()) {
        return None;
    }
    let string = &*value;
    if string.hdr.kind != LIRA_KIND_STRING
        || !(0..=MAX_ABI_BYTES).contains(&string.len)
        || (string.len as usize) > isize::MAX as usize
    {
        return None;
    }
    let bytes = slice::from_raw_parts(string.data.as_ptr(), string.len as usize);
    str::from_utf8(bytes).ok()
}

unsafe fn new_str(value: &str) -> *mut LiraStr {
    lira_rt_str_new(value.as_ptr().cast::<c_char>(), value.len() as i64)
}

unsafe fn panic_runtime(message: &str) -> ! {
    let mut message = message.as_bytes().to_vec();
    message.push(0);
    lira_rt_panic(message.as_ptr().cast::<c_char>());
    // The bundled C runtime exits.  Keep this boundary non-returning even if
    // a test double unexpectedly returns from lira_rt_panic.
    std::process::abort()
}

fn invalid_abi() -> ! {
    // This is an ABI violation, not a JSON parse failure.
    unsafe { panic_runtime("invalid Lira JSON string") }
}

#[derive(Debug)]
enum BuildError {
    SizeLimit,
    ResourceLimit,
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

/// Preflight JSON structure iteratively so serde_json never sees input deeper
/// than the runtime's configured bound. Syntax validation remains serde_json's
/// responsibility after this resource check.
fn check_json_resources(input: &str) -> Result<(), BuildError> {
    if input.len() > MAX_JSON_BYTES {
        return Err(BuildError::SizeLimit);
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
                if frames.len() > MAX_JSON_DEPTH || nodes >= MAX_JSON_NODES {
                    return Err(BuildError::ResourceLimit);
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
                if frames.len() > MAX_JSON_DEPTH || nodes >= MAX_JSON_NODES {
                    return Err(BuildError::ResourceLimit);
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
            Some(_) => index += 1,
            None if root_done => break,
            None => {
                if nodes >= MAX_JSON_NODES {
                    return Err(BuildError::ResourceLimit);
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

struct BuildState {
    nodes: usize,
}

impl BuildState {
    fn visit(&mut self, depth: usize) -> Result<(), BuildError> {
        if depth > MAX_JSON_DEPTH || self.nodes >= MAX_JSON_NODES {
            return Err(BuildError::ResourceLimit);
        }
        self.nodes += 1;
        Ok(())
    }
}

unsafe fn value_to_any(
    value: &Value,
    depth: usize,
    state: &mut BuildState,
) -> Result<*mut LiraAny, BuildError> {
    state.visit(depth)?;
    match value {
        Value::Null => Ok(lira_rt_any_null()),
        Value::Bool(value) => Ok(lira_rt_any_box_bool(*value as i8)),
        Value::Number(value) => {
            if let Some(integer) = value.as_i64() {
                Ok(lira_rt_any_box_int(integer))
            } else {
                // serde_json numbers are finite when represented as f64.  A
                // number outside f64's range is rejected instead of silently
                // turning into an infinity which JSON cannot represent.
                value
                    .as_f64()
                    .filter(|float| float.is_finite())
                    .map(|float| lira_rt_any_box_float(float))
                    .ok_or(BuildError::ResourceLimit)
            }
        }
        Value::String(value) => {
            if value.len() > MAX_JSON_OUTPUT_BYTES {
                return Err(BuildError::ResourceLimit);
            }
            Ok(lira_rt_any_box_string(new_str(value)))
        }
        Value::Array(values) => {
            let array = lira_rt_array_new(values.len().min(i64::MAX as usize) as i64);
            for value in values {
                let element = value_to_any(value, depth + 1, state)?;
                lira_rt_array_push(array, element as usize as i64);
            }
            Ok(lira_rt_any_box_array(array))
        }
        Value::Object(values) => {
            let object = lira_rt_map_new();
            for (key, value) in values {
                if key.len() > MAX_JSON_OUTPUT_BYTES {
                    return Err(BuildError::ResourceLimit);
                }
                let key = new_str(key);
                let element = value_to_any(value, depth + 1, state)?;
                lira_rt_map_set(object, key, element as usize as i64);
            }
            Ok(lira_rt_any_box_map(object))
        }
    }
}

#[derive(Debug)]
enum ConvertError {
    ResourceLimit,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum AggregateIdentity {
    Array(*mut LiraArray),
    Object(*mut c_void),
}

struct ConvertState {
    nodes: usize,
    active: HashSet<AggregateIdentity>,
}

impl ConvertState {
    fn visit(&mut self, depth: usize) -> Result<(), ConvertError> {
        if depth > MAX_JSON_DEPTH || self.nodes >= MAX_JSON_NODES {
            return Err(ConvertError::ResourceLimit);
        }
        self.nodes += 1;
        Ok(())
    }
}

unsafe fn any_to_value(
    value: *const LiraAny,
    depth: usize,
    state: &mut ConvertState,
) -> Result<Value, ConvertError> {
    if value.is_null() || !(value as usize).is_multiple_of(std::mem::align_of::<LiraAny>()) {
        return Ok(Value::Null);
    }
    state.visit(depth)?;
    let any = &*value;
    if any.hdr.kind != LIRA_KIND_ANY {
        return Ok(Value::Null);
    }
    match any.tag {
        LIRA_ANY_NULL => Ok(Value::Null),
        LIRA_ANY_BOOL => Ok(Value::Bool(any.payload != 0)),
        LIRA_ANY_INT => Ok(Value::Number((any.payload as i64).into())),
        LIRA_ANY_FLOAT => {
            let float = f64::from_bits(any.payload);
            Ok(Number::from_f64(float)
                .map(Value::Number)
                .unwrap_or(Value::Null))
        }
        LIRA_ANY_STRING => {
            let string = any.payload as *const LiraStr;
            Ok(read_str(string)
                .map(str::to_owned)
                .map(Value::String)
                .unwrap_or(Value::Null))
        }
        LIRA_ANY_ARRAY => {
            let array = any.payload as *mut LiraArray;
            let len = lira_rt_array_len(array);
            if len < 0 || len as usize > MAX_JSON_NODES {
                return Err(ConvertError::ResourceLimit);
            }
            if !state.active.insert(AggregateIdentity::Array(array)) {
                return Ok(Value::Null);
            }
            let mut result = Vec::with_capacity(len as usize);
            for index in 0..len {
                result.push(any_to_value(
                    lira_rt_any_array_at(value, index),
                    depth + 1,
                    state,
                )?);
            }
            state.active.remove(&AggregateIdentity::Array(array));
            Ok(Value::Array(result))
        }
        LIRA_ANY_OBJECT => {
            let object = any.payload as *mut c_void;
            let len = lira_rt_map_len(object);
            if len < 0 || len as usize > MAX_JSON_NODES {
                return Err(ConvertError::ResourceLimit);
            }
            let keys = lira_rt_map_keys(object);
            if lira_rt_array_len(keys) != len {
                return Err(ConvertError::ResourceLimit);
            }
            if !state.active.insert(AggregateIdentity::Object(object)) {
                return Ok(Value::Null);
            }
            let mut result = Map::new();
            for index in 0..len {
                let key_ptr = lira_rt_array_get(keys, index) as *const LiraStr;
                let Some(key) = read_str(key_ptr) else {
                    state.active.remove(&AggregateIdentity::Object(object));
                    return Ok(Value::Null);
                };
                let field = lira_rt_any_object_at(value, key_ptr);
                result.insert(key.to_owned(), any_to_value(field, depth + 1, state)?);
            }
            state.active.remove(&AggregateIdentity::Object(object));
            Ok(Value::Object(result))
        }
        // Values without a JSON representation become JSON null, matching
        // the bytecode VM's value_to_json contract.
        LIRA_ANY_REF | LIRA_ANY_FUNCTION | LIRA_ANY_CHANNEL | LIRA_ANY_FIBER => Ok(Value::Null),
        _ => Ok(Value::Null),
    }
}

unsafe fn stringify(value: *const LiraAny, pretty: bool) -> *mut LiraStr {
    let mut state = ConvertState {
        nodes: 0,
        active: HashSet::new(),
    };
    let json = any_to_value(value, 0, &mut state).unwrap_or(Value::Null);
    let output = if pretty {
        serde_json::to_string_pretty(&json).unwrap_or_else(|_| "null".to_owned())
    } else {
        serde_json::to_string(&json).unwrap_or_else(|_| "null".to_owned())
    };
    if output.len() > MAX_JSON_OUTPUT_BYTES {
        return new_str("null");
    }
    new_str(&output)
}

/// Parse a JSON string into native `LiraAny` values.
#[no_mangle]
pub extern "C" fn lira_rt_json_parse(value: *const LiraStr) -> *mut LiraAny {
    match catch_unwind(AssertUnwindSafe(|| unsafe {
        let Some(input) = read_str(value) else {
            invalid_abi()
        };
        if let Err(error) = check_json_resources(input) {
            let message = match error {
                BuildError::SizeLimit => JSON_SIZE_LIMIT_ERROR,
                BuildError::ResourceLimit => JSON_RESOURCE_LIMIT_ERROR,
            };
            eprintln!("json_parse error: {message}");
            return lira_rt_any_null();
        }
        let parsed = match serde_json::from_str::<Value>(input) {
            Ok(value) => value,
            Err(error) => {
                // Match Runtime::json_parse plus the VM syscall wrapper.
                eprintln!("json_parse error: JSON parse error: {error}");
                return lira_rt_any_null();
            }
        };
        let mut state = BuildState { nodes: 0 };
        match value_to_any(&parsed, 0, &mut state) {
            Ok(value) => value,
            Err(BuildError::SizeLimit) => {
                eprintln!("json_parse error: {JSON_SIZE_LIMIT_ERROR}");
                lira_rt_any_null()
            }
            Err(BuildError::ResourceLimit) => {
                eprintln!("json_parse error: {JSON_RESOURCE_LIMIT_ERROR}");
                lira_rt_any_null()
            }
        }
    })) {
        Ok(value) => value,
        Err(_) => unsafe { panic_runtime("json runtime panic") },
    }
}

/// Serialize a native `LiraAny` value using compact JSON notation.
#[no_mangle]
pub extern "C" fn lira_rt_json_stringify(value: *const LiraAny) -> *mut LiraStr {
    match catch_unwind(AssertUnwindSafe(|| unsafe { stringify(value, false) })) {
        Ok(value) => value,
        Err(_) => unsafe { panic_runtime("json runtime panic") },
    }
}

/// Serialize a native `LiraAny` value using serde_json's pretty notation.
#[no_mangle]
pub extern "C" fn lira_rt_json_stringify_pretty(value: *const LiraAny) -> *mut LiraStr {
    match catch_unwind(AssertUnwindSafe(|| unsafe { stringify(value, true) })) {
        Ok(value) => value,
        Err(_) => unsafe { panic_runtime("json runtime panic") },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_preflight_matches_depth_and_node_limits() {
        let too_deep = format!(
            "{}0{}",
            "[".repeat(MAX_JSON_DEPTH + 1),
            "]".repeat(MAX_JSON_DEPTH + 1)
        );
        assert!(matches!(
            check_json_resources(&too_deep),
            Err(BuildError::ResourceLimit)
        ));

        let too_many_nodes = format!(
            "[{}]",
            (0..=MAX_JSON_NODES).map(|_| "0,").collect::<String>()
        );
        assert!(matches!(
            check_json_resources(&too_many_nodes),
            Err(BuildError::ResourceLimit)
        ));
    }

    #[test]
    fn resource_preflight_reports_input_size_limit() {
        let input = "x".repeat(MAX_JSON_BYTES + 1);
        assert!(matches!(
            check_json_resources(&input),
            Err(BuildError::SizeLimit)
        ));
    }
}
