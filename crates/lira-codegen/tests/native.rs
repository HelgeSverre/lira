//! End-to-end tests for the native backend.
//!
//! Each test compiles a Lira program to a real executable, runs it, and checks
//! its output. Going through the linker rather than the JIT is deliberate: it
//! covers object emission and linking too, and it keeps the C runtime's
//! single-threaded scheduler state in a process of its own, which the test
//! harness's thread pool would otherwise share.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Compile `source` and return everything it wrote to stdout and stderr.
fn run_native(source: &str) -> Result<String, String> {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let source_path = dir.join("program.li");
    let binary_path = dir.join("program");
    std::fs::write(&source_path, source).expect("write source");

    let result = (|| {
        lira_codegen::build_native(
            source_path.to_str().expect("utf-8 path"),
            source,
            &binary_path,
        )?;
        let output = Command::new(&binary_path)
            .output()
            .map_err(|e| format!("could not run the compiled program: {}", e))?;
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Ok(text)
    })();

    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn scratch_dir() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("lira-native-test-{}-{}", std::process::id(), id))
}

/// Assert that a program prints exactly these lines.
#[track_caller]
fn assert_lines(source: &str, expected: &[&str]) {
    let output = run_native(source).unwrap_or_else(|e| panic!("compilation failed: {}", e));
    let actual: Vec<&str> = output.lines().collect();
    assert_eq!(actual, expected, "\n--- program ---\n{}", source);
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
        fn producer(ch) {
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
fn a_blocked_program_reports_a_deadlock_instead_of_hanging() {
    let output = run_native(
        r#"
        fn main() {
            let ch = chan(0)
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
        var total = 0
        for i in r { total = total + i }
        println(total)
        let inclusive = 1..=4
        var sum = 0
        for i in inclusive { sum = sum + i }
        println(sum)
        "#,
        &["1", "4", "false", "6", "10"],
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
// Diagnostics                                                             //
// ---------------------------------------------------------------------- //

#[test]
fn unsupported_constructs_are_refused_rather_than_mis_compiled() {
    assert_rejected(
        "fn identity<T>(x: T) -> T { return x }\nprintln(identity(1))",
        "native backend",
    );
}

#[test]
fn a_type_error_stops_native_compilation() {
    assert_rejected("let x: int = \"not an int\"", "");
}

#[test]
fn the_error_points_at_the_bytecode_vm_as_the_fallback() {
    match run_native("fn identity<T>(x: T) -> T { return x }\nprintln(identity(1))") {
        Ok(_) => panic!("expected a compile error"),
        Err(error) => assert!(
            error.contains("lira run"),
            "the error should name the working alternative: {}",
            error
        ),
    }
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
    let binary = dir.join("prog");
    lira_codegen::build_native("t.li", "println(\"hi\")", &binary).expect("builds");
    assert!(Path::new(&binary).exists());
    let output = Command::new(&binary).output().expect("runs");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "hi\n");
    let _ = std::fs::remove_dir_all(&dir);
}
