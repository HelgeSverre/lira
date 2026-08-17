use super::*;
use std::ffi::c_char;
use std::ptr;

#[repr(C)]
struct TestArray {
    hdr: LiraHeader,
    len: i64,
    cap: i64,
    data: *mut i64,
}

unsafe fn test_string(value: &str) -> *mut LiraStr {
    let mut bytes = vec![0_u8; 25 + value.len()];
    let ptr = bytes.as_mut_ptr();
    std::mem::forget(bytes);
    let string = ptr.cast::<LiraStr>();
    (*string).hdr = LiraHeader {
        kind: LIRA_KIND_STRING,
        flags: 0,
        rc: 1,
    };
    (*string).len = value.len() as i64;
    ptr::copy_nonoverlapping(value.as_ptr(), (*string).data.as_mut_ptr(), value.len());
    *(*string).data.as_mut_ptr().add(value.len()) = 0;
    string
}

unsafe fn test_string_value(value: *const LiraStr) -> String {
    let len = (*value).len as usize;
    let bytes = std::slice::from_raw_parts((*value).data.as_ptr(), len);
    String::from_utf8_lossy(bytes).into_owned()
}

unsafe fn test_array_values(value: *const LiraArray) -> Vec<String> {
    let array = &*(value.cast::<TestArray>());
    let values = std::slice::from_raw_parts(array.data, array.len as usize);
    values
        .iter()
        .map(|pointer| test_string_value(*pointer as usize as *const LiraStr))
        .collect()
}

#[no_mangle]
unsafe extern "C" fn lira_rt_str_new(bytes: *const c_char, len: i64) -> *mut LiraStr {
    let bytes = if len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(bytes.cast::<u8>(), len as usize)
    };
    let value = std::str::from_utf8(bytes).unwrap_or_default();
    test_string(value)
}

#[no_mangle]
unsafe extern "C" fn lira_rt_array_new(cap: i64) -> *mut LiraArray {
    let cap = cap.max(0) as usize;
    let mut values = vec![0_i64; cap];
    let data = values.as_mut_ptr();
    std::mem::forget(values);
    let array = Box::new(TestArray {
        hdr: LiraHeader {
            kind: 2,
            flags: 0,
            rc: 1,
        },
        len: 0,
        cap: cap as i64,
        data,
    });
    Box::into_raw(array).cast()
}

#[no_mangle]
unsafe extern "C" fn lira_rt_array_push(array: *mut LiraArray, value: i64) {
    let array = &mut *(array.cast::<TestArray>());
    if array.len == array.cap {
        let old_cap = array.cap as usize;
        let mut values = Vec::from_raw_parts(array.data, old_cap, old_cap);
        let next_cap = old_cap.max(1) * 2;
        values.resize(next_cap, 0);
        array.data = values.as_mut_ptr();
        array.cap = next_cap as i64;
        std::mem::forget(values);
    }
    *array.data.add(array.len as usize) = value;
    array.len += 1;
}

#[no_mangle]
unsafe extern "C" fn lira_rt_panic(_message: *const c_char) {}

fn strings(values: &[&str]) -> Vec<*mut LiraStr> {
    values
        .iter()
        .map(|value| unsafe { test_string(value) })
        .collect()
}

#[test]
fn unicode_shorthands_and_properties_match_regex_crate() {
    let [word, property] = strings(&[r"\w+", r"\p{Greek}+"]).try_into().unwrap();
    let [greek, letters] = strings(&["γειά", "Ж"]).try_into().unwrap();
    assert_eq!(lira_rt_regex_match(word, greek), 1);
    assert_eq!(lira_rt_regex_match(property, greek), 1);
    assert_eq!(lira_rt_regex_match(word, letters), 1);
}

#[test]
fn captures_include_full_match_and_present_named_groups_only() {
    let [pattern, text] = strings(&[r"(?P<word>[a-z]+)(?:-([0-9]+))?", "abc"])
        .try_into()
        .unwrap();
    let values = unsafe { test_array_values(lira_rt_regex_captures(pattern, text)) };
    assert_eq!(values, ["abc", "abc"]);

    let [optional, b] = strings(&[r"(a)?(b)", "b"]).try_into().unwrap();
    let values = unsafe { test_array_values(lira_rt_regex_captures(optional, b)) };
    assert_eq!(values, ["b", "b"]);
}

#[test]
fn zero_width_matches_and_split_preserve_regex_semantics() {
    let [anchors, text] = strings(&[r"^|$", "ab"]).try_into().unwrap();
    let values = unsafe { test_array_values(lira_rt_regex_find_all(anchors, text)) };
    assert_eq!(values, ["", ""]);

    let [separator, input] = strings(&[r"[,;]", "a,b;c,"]).try_into().unwrap();
    let values = unsafe { test_array_values(lira_rt_regex_split(separator, input)) };
    assert_eq!(values, ["a", "b", "c", ""]);
}

#[test]
fn replacements_support_shorthand_named_and_multi_digit_groups() {
    let [pattern, text, replacement] = strings(&[r"([a-z]+)([0-9]+)", "abc12 xyz34", "$2-$1"])
        .try_into()
        .unwrap();
    let output = unsafe { test_string_value(lira_rt_regex_replace(pattern, text, replacement)) };
    assert_eq!(output, "12-abc xyz34");

    let [digits, input, dollar] = strings(&[r"[0-9]+", "a1b22", "[$0]/$$"])
        .try_into()
        .unwrap();
    let output = unsafe { test_string_value(lira_rt_regex_replace_all(digits, input, dollar)) };
    assert_eq!(output, "a[1]/$b[22]/$");

    let group_pattern = r"(?P<name>[a-z]+)-([0-9]+)";
    let [named_pattern, named_text, named_replacement] =
        strings(&[group_pattern, "abc-42", "${name}:$2"])
            .try_into()
            .unwrap();
    let output = unsafe {
        test_string_value(lira_rt_regex_replace_all(
            named_pattern,
            named_text,
            named_replacement,
        ))
    };
    assert_eq!(output, "abc:42");

    let many_groups = r"(0)(1)(2)(3)(4)(5)(6)(7)(8)(9)";
    let [many, many_text, many_replacement] = strings(&[many_groups, "0123456789", "$10-$1"])
        .try_into()
        .unwrap();
    let output =
        unsafe { test_string_value(lira_rt_regex_replace(many, many_text, many_replacement)) };
    assert_eq!(output, "9-0");
}

#[test]
fn invalid_patterns_use_vm_fallbacks() {
    let [invalid, text, replacement] = strings(&["[", "abc", "X"]).try_into().unwrap();
    assert_eq!(lira_rt_regex_is_valid(invalid), 0);
    assert_eq!(lira_rt_regex_match(invalid, text), 0);
    assert_eq!(
        unsafe { test_string_value(lira_rt_regex_find(invalid, text)) },
        ""
    );
    assert!(unsafe { test_array_values(lira_rt_regex_find_all(invalid, text)) }.is_empty());
    assert_eq!(
        unsafe { test_string_value(lira_rt_regex_replace(invalid, text, replacement)) },
        "abc"
    );
    assert_eq!(
        unsafe { test_string_value(lira_rt_regex_replace_all(invalid, text, replacement)) },
        "abc"
    );
    assert_eq!(
        unsafe { test_array_values(lira_rt_regex_split(invalid, text)) },
        ["abc"]
    );
    assert!(unsafe { test_array_values(lira_rt_regex_captures(invalid, text)) }.is_empty());
}

#[test]
fn malformed_abi_inputs_take_fallbacks_without_dereferencing_invalid_slices() {
    assert_eq!(lira_rt_regex_match(std::ptr::null(), std::ptr::null()), 0);
    let malformed = LiraStr {
        hdr: LiraHeader {
            kind: LIRA_KIND_STRING,
            flags: 0,
            rc: 1,
        },
        len: -1,
        data: [0],
    };
    assert_eq!(lira_rt_regex_is_valid(&malformed), 0);
}

#[test]
fn regex_resource_limits_match_vm_fallbacks() {
    unsafe {
        let pattern = test_string(&"a".repeat(MAX_REGEX_PATTERN_BYTES + 1));
        let input = test_string(&"a".repeat(MAX_REGEX_INPUT_BYTES + 1));
        assert_eq!(lira_rt_regex_is_valid(pattern), 0);
        assert_eq!(lira_rt_regex_match(test_string("a"), input), 0);
        assert_eq!(
            test_string_value(lira_rt_regex_find(test_string("a"), input)),
            ""
        );
        assert!(test_array_values(lira_rt_regex_find_all(test_string("a"), input)).is_empty());
        assert_eq!(
            test_string_value(lira_rt_regex_replace(
                test_string("a"),
                input,
                test_string("x")
            )),
            test_string_value(input)
        );
        assert_eq!(
            test_array_values(lira_rt_regex_split(test_string("a"), input)),
            [test_string_value(input)]
        );

        let many = test_string(&"a".repeat(MAX_REGEX_RESULT_COUNT + 1));
        assert!(test_array_values(lira_rt_regex_find_all(test_string("a"), many)).is_empty());
        assert_eq!(
            test_array_values(lira_rt_regex_split(test_string("a"), many)),
            [test_string_value(many)]
        );
    }
}

#[test]
fn regex_replacement_output_limit_preserves_input() {
    unsafe {
        let input_text = "a".repeat(4 * 1024 * 1024);
        let replacement_text = "x".repeat(9);
        let input = test_string(&input_text);
        let replacement = test_string(&replacement_text);
        let output = test_string_value(lira_rt_regex_replace_all(
            test_string("a"),
            input,
            replacement,
        ));
        assert_eq!(output, input_text);
    }
}
