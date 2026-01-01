# Lira Programming Language

Welcome to the Lira programming language documentation.

Lira is a modern systems programming language with fiber concurrency, pattern matching, and a custom bytecode VM.

## Standard Library

- [collections](standard-library/collections.md)
- [core](standard-library/core.md)
- [env](standard-library/env.md)
- [fs](standard-library/fs.md)
- [hash](standard-library/hash.md)
- [http](standard-library/http.md)
- [io](standard-library/io.md)
- [log](standard-library/log.md)
- [math](standard-library/math.md)
- [net](standard-library/net.md)
- [os](standard-library/os.md)
- [path](standard-library/path.md)
- [random](standard-library/random.md)
- [regex](standard-library/regex.md)
- [strings](standard-library/strings.md)
- [test](standard-library/test.md)
- [time](standard-library/time.md)
- [url](standard-library/url.md)
- [uuid](standard-library/uuid.md)

## Examples

- [all_binary_ops](examples/all_binary_ops.md)
- [arithmetic_edge_cases](examples/arithmetic_edge_cases.md)
- [array_ops](examples/array_ops.md)
- [array_types](examples/array_types.md)
- [bitwise_ops](examples/bitwise_ops.md)
- [block_expressions](examples/block_expressions.md)
- [channel_basic](examples/channel_basic.md)
- [char_literals](examples/char_literals.md)
- [class_inheritance](examples/class_inheritance.md)
- [classes_basic](examples/classes_basic.md)
- [compound_assign](examples/compound_assign.md)
- [const_declarations](examples/const_declarations.md)
- [control_flow](examples/control_flow.md)
- [control_flow_test](examples/control_flow_test.md)
- [default_params](examples/default_params.md)
- [enum_data](examples/enum_data.md)
- [enums_basic](examples/enums_basic.md)
- [factorial](examples/factorial.md)
- [factorial_debug](examples/factorial_debug.md)
- [fiber_basic](examples/fiber_basic.md)
- [fibonacci](examples/fibonacci.md)
- [file_io](examples/file_io.md)
- [for_loop](examples/for_loop.md)
- [function_types](examples/function_types.md)
- [generics_basic](examples/generics_basic.md)
- [hello](examples/hello.md)
- [if_expressions](examples/if_expressions.md)
- [impl_block](examples/impl_block.md)
- [import_selective](examples/import_selective.md)
- [import_test](examples/import_test.md)
- [integer_types](examples/integer_types.md)
- [interface_basic](examples/interface_basic.md)
- [lambda](examples/lambda.md)
- [loop_control](examples/loop_control.md)
- [loop_infinite](examples/loop_infinite.md)
- [math_test](examples/math_test.md)
- [method_chaining](examples/method_chaining.md)
- [module_comprehensive](examples/module_comprehensive.md)
- [mutual_recursion](examples/mutual_recursion.md)
- [named_arguments](examples/named_arguments.md)
- [nested_structures](examples/nested_structures.md)
- [null_and_optionals](examples/null_and_optionals.md)
- [operator_comprehensive](examples/operator_comprehensive.md)
- [optional_access](examples/optional_access.md)
- [optional_chaining](examples/optional_chaining.md)
- [pattern_constructor](examples/pattern_constructor.md)
- [pattern_constructor_verify](examples/pattern_constructor_verify.md)
- [pattern_guards](examples/pattern_guards.md)
- [pattern_match](examples/pattern_match.md)
- [pattern_tuple](examples/pattern_tuple.md)
- [pattern_tuple_simple](examples/pattern_tuple_simple.md)
- [power_operator](examples/power_operator.md)
- [prime_checker](examples/prime_checker.md)
- [range_expressions](examples/range_expressions.md)
- [recursion_stress](examples/recursion_stress.md)
- [result_propagation](examples/result_propagation.md)
- [select_basic](examples/select_basic.md)
- [smoke_test_fs](examples/smoke_test_fs.md)
- [spawn_expression](examples/spawn_expression.md)
- [stdlib_demo](examples/stdlib_demo.md)
- [string_escapes](examples/string_escapes.md)
- [string_ops](examples/string_ops.md)
- [structs](examples/structs.md)
- [test_base64](examples/test_base64.md)
- [test_collections](examples/test_collections.md)
- [test_env](examples/test_env.md)
- [test_hash](examples/test_hash.md)
- [test_http](examples/test_http.md)
- [test_json](examples/test_json.md)
- [test_log](examples/test_log.md)
- [test_math](examples/test_math.md)
- [test_net](examples/test_net.md)
- [test_os](examples/test_os.md)
- [test_path](examples/test_path.md)
- [test_random](examples/test_random.md)
- [test_regex](examples/test_regex.md)
- [test_string](examples/test_string.md)
- [test_test](examples/test_test.md)
- [test_time](examples/test_time.md)
- [test_url](examples/test_url.md)
- [test_uuid](examples/test_uuid.md)
- [traits_basic](examples/traits_basic.md)
- [try_operator](examples/try_operator.md)
- [tuple_types](examples/tuple_types.md)
- [type_alias](examples/type_alias.md)
- [type_expressions](examples/type_expressions.md)
- [unary_operators](examples/unary_operators.md)

## Building the Documentation

```bash
mdbook build
mdbook serve  # For local preview
```

## Rust API Documentation

For the Rust implementation details, generate API docs with:

```bash
cargo doc --open
```
