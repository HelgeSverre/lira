//! Robustness fuzzer: the compiler must never *panic* — only return `Ok`/`Err`.
//!
//! Feeds random token soup drawn from Lira's own vocabulary through the full
//! front end (lex → parse → check → codegen) and asserts it never unwinds.
//! A panic here is a crash bug (unwrap on malformed input, index-out-of-bounds,
//! unbounded recursion, ...). Compile-only on purpose: running arbitrary
//! generated programs could loop forever; bounded *execution* is covered by the
//! differential fuzzer.

use proptest::prelude::*;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// A vocabulary biased toward real Lira syntax so the parser gets deep into
/// the grammar rather than dying at the first token.
const TOKENS: &[&str] = &[
    "fn", "let", "var", "main", "struct", "impl", "trait", "enum", "for", "if", "else", "while",
    "match", "return", "spawn", "select", "import", "true", "false", "null", "self", "int",
    "string", "bool", "println", "chan", "send", "recv", "push", "pop", "len", "x", "y", "foo",
    "0", "1", "42", "-7", "3.14", "\"s\"", "(", ")", "{", "}", "[", "]", "=", "==", "!=", "+", "-",
    "*", "/", "%", "<", ">", "&&", "||", "!", ":", ",", ".", ";", "->", "=>", "?", "??", "?.", "|",
    "<-", "::", "@", "#", "\\", "\n",
];

fn source() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(TOKENS), 0..48).prop_map(|toks| toks.join(" "))
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 4000,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn compiler_never_panics(src in source()) {
        let result = catch_unwind(AssertUnwindSafe(|| {
            // Whatever it returns is fine; we only forbid an unwind.
            let _ = lirac::compile_with_imports("fuzz.li", &src);
        }));
        prop_assert!(result.is_ok(), "compiler panicked on input:\n{}", src);
    }
}
