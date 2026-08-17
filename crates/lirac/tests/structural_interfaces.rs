//! Checker-level coverage for non-generic structural interfaces.

use lirac::checker::Type;
use lirac::{analyze, check, parse_source};

fn run_vm(source: &str) -> Vec<String> {
    let bytecode = lirac::compile(source).expect("interface source should compile to bytecode");
    let (status, output) =
        liravm::run_with_capture(&bytecode).expect("compiled interface source should run");
    assert_eq!(status, 0, "interface source should exit successfully");
    output
}

#[test]
fn struct_methods_satisfy_interface_parameters_and_returns() {
    let source = r#"
interface Named { fn name() -> string }
struct User {
    fn name(self) -> string { return "user" }
}
fn accept(value: Named) -> Named { return value }
fn make() -> Named { return User {} }
let named: Named = User {}
accept(named)
make()
"#;

    check(source).expect("structural interface assignment should type-check");
}

#[test]
fn struct_interface_values_dispatch_after_argument_and_return_conversion() {
    let output = run_vm(
        r#"
interface Named { fn name() -> string }
struct User {
    fn name(self) -> string { return "user" }
}
fn identity(value: Named) -> Named { return value }
println(identity(User {}).name())
"#,
    );

    assert_eq!(output, vec!["user"]);
}

#[test]
fn interface_methods_normalize_explicit_and_implicit_receivers() {
    let source = r#"
interface Explicit {
    fn value(this, amount: int = 1) -> Self
}
interface Implicit {
    fn value(amount: int = 1) -> Self
}
fn call_explicit(value: Explicit) -> Explicit { return value.value() }
fn call_implicit(value: Implicit) -> Implicit { return value.value() }
"#;
    let program = parse_source(source).expect("interface source should parse");
    let checked = lirac::checker::check(&program).expect("Self should resolve in interface scope");

    for (name, expected_receiver) in [("Explicit", "Explicit"), ("Implicit", "Implicit")] {
        let methods = checked
            .sema
            .type_members
            .get(name)
            .expect("interface methods are snapshotted");
        let method = methods.methods.first().expect("interface method");
        let Type::Function {
            params,
            required_params,
            ..
        } = &method.ty
        else {
            panic!("interface method must be a function");
        };
        assert_eq!(
            params.first(),
            Some(&Type::Interface(expected_receiver.to_string()))
        );
        assert_eq!(*required_params, 1);
    }
}

#[test]
fn inherited_class_methods_satisfy_interface() {
    let source = r#"
interface Named { fn name() -> string }
class Base { fn name(self) -> string { return "base" } }
class Child extends Base {}
fn accept(value: Named) {}
accept(Child {})
"#;
    let program = parse_source(source).expect("interface source should parse");
    let checked = lirac::checker::check(&program)
        .expect("inherited instance method should satisfy interface");
    assert!(checked
        .sema
        .type_members
        .get("Child")
        .is_some_and(|members| members.methods.iter().any(|method| method.name == "name")));
}

#[test]
fn interfaces_are_structurally_width_subtyped() {
    let source = r#"
interface Narrow { fn first() -> int }
interface Wide { fn first() -> int fn second() -> int }
fn accept(value: Narrow) {}
fn pass(value: Wide) { accept(value) }
"#;
    check(source)
        .expect("an interface with more guaranteed methods should satisfy a narrower interface");
}

#[test]
fn semantic_tables_publish_checker_verified_runtime_conformers() {
    let source = r#"
interface Named { fn name() -> string }
interface Sized { fn len() -> int }
struct User { fn name(self) -> string { return "user" } }
struct Missing {}
let values = [1, 2]
"#;
    let program = parse_source(source).expect("interface source should parse");
    let checked = lirac::checker::check(&program).expect("interface source should check");

    let named = checked
        .sema
        .interface_implementations
        .get("Named")
        .expect("Named implementation set");
    assert!(named.contains(&Type::Struct("User".to_string())));
    assert!(named.contains(&Type::Interface("Named".to_string())));
    assert!(!named.contains(&Type::Struct("Missing".to_string())));
    let sized = checked
        .sema
        .interface_implementations
        .get("Sized")
        .expect("Sized implementation set");
    assert!(sized.contains(&Type::String));
    assert!(sized.contains(&Type::Array(Box::new(Type::Int))));
    assert!(!sized.contains(&Type::Struct("Missing".to_string())));
}

#[test]
fn generic_and_nested_values_retain_structural_interface_types() {
    let source = r#"
interface IntValue { fn value() -> int }
struct Box<T> {
    item: T
    fn value(self) -> T { return self.item }
}
fn accept(value: IntValue?) -> (IntValue, IntValue?) {
    return (Box { item: 1 }, value)
}
let nested: (IntValue, IntValue?) = accept(Box { item: 2 })
"#;

    check(source).expect("generic methods and nested interface positions should be resolved");
}

#[test]
fn implementation_must_accept_every_interface_defaulted_call() {
    let source = r#"
interface Flexible { fn value(amount: int = 1) -> int }
struct Required {
    fn value(self, amount: int) -> int { return amount }
}
struct Optional {
    fn value(self, amount: int = 2) -> int { return amount }
}
fn accept(value: Flexible) {}
accept(Required {})
accept(Optional {})
"#;

    let diagnostics = analyze(source).expect("source should parse").diagnostics;
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("Type 'Required' does not satisfy interface 'Flexible'")
    }));
    assert!(!diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("Type 'Optional' does not satisfy interface 'Flexible'")
    }));
}

#[test]
fn impl_method_defaults_participate_in_interface_compatibility() {
    let source = r#"
interface Flexible { fn value(amount: int = 1) -> int }
struct Required {}
impl Required {
    fn value(self, amount: int) -> int { return amount }
}
struct Optional {}
impl Optional {
    fn value(self, amount: int = 2) -> int { return amount }
}
fn accept(value: Flexible) {}
accept(Required {})
accept(Optional {})
"#;

    let diagnostics = analyze(source).expect("source should parse").diagnostics;
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("Type 'Required' does not satisfy interface 'Flexible'")
    }));
    assert!(!diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("Type 'Optional' does not satisfy interface 'Flexible'")
    }));
}

#[test]
fn implementation_defaults_must_cover_the_same_parameter_slots() {
    let source = r#"
interface Trailing { fn value(first: int, second: int = 2) -> int }
struct WrongSlot {
    fn value(self, first: int = 1, second: int) -> int { return second }
}
struct SameSlot {
    fn value(self, first: int, second: int = 3) -> int { return first + second }
}
fn accept(value: Trailing) {}
accept(WrongSlot {})
accept(SameSlot {})
"#;

    let diagnostics = analyze(source).expect("source should parse").diagnostics;
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("Type 'WrongSlot' does not satisfy interface 'Trailing'")
    }));
    assert!(!diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("Type 'SameSlot' does not satisfy interface 'Trailing'")
    }));
}

#[test]
fn missing_wrong_and_static_methods_are_rejected() {
    let source = r#"
interface Required { fn value(amount: int) -> int }
struct Missing {}
struct Wrong { fn value(self, amount: string) -> string { return amount } }
struct Static { fn value(amount: int) -> int { return amount } }
fn accept(value: Required) {}
accept(Missing {})
accept(Wrong {})
accept(Static {})
"#;
    let diagnostics = analyze(source).expect("source should parse").diagnostics;
    assert!(diagnostics
        .iter()
        .any(|d| d.message.contains("missing instance method 'value'")));
    assert!(diagnostics.iter().any(|d| d
        .message
        .contains("method 'value' has an incompatible signature")));
}

#[test]
fn duplicate_interface_and_inline_methods_are_rejected() {
    let source = r#"
interface DuplicateInterface {
    fn value() -> int
    fn value() -> string
}
struct DuplicateStruct {
    fn value(self) -> int { return 1 }
    fn value(self) -> string { return "wrong" }
}
class DuplicateClass {
    fn value(self) -> int { return 1 }
    fn value(self) -> string { return "wrong" }
}
"#;

    let diagnostics = analyze(source).expect("source should parse").diagnostics;
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("Duplicate method 'value' in interface 'DuplicateInterface'")
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("Duplicate method 'value' in type 'DuplicateStruct'")
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("Duplicate method 'value' in type 'DuplicateClass'")
    }));
}

#[test]
fn explicit_class_interface_annotations_validate_names_and_signatures() {
    let source = r#"
interface Required { fn value() -> int }
class Good : Required { fn value(self) -> int { return 1 } }
class Missing : Required {}
class Wrong : Required { fn value(self) -> string { return "wrong" } }
class NotAnInterface {}
class DeclaresClass : NotAnInterface {}
class Unknown : MissingInterface {}
"#;
    let diagnostics = analyze(source).expect("source should parse").diagnostics;
    assert!(diagnostics
        .iter()
        .any(|d| d.message.contains("Class 'Missing'")));
    assert!(diagnostics
        .iter()
        .any(|d| d.message.contains("Class 'Wrong'")));
    assert!(diagnostics
        .iter()
        .any(|d| d.message.contains("not an interface")));
    assert!(diagnostics
        .iter()
        .any(|d| d.message.contains("unknown interface")));
}

#[test]
fn interface_casts_use_structural_compatibility() {
    let source = r#"
interface Named { fn name() -> string }
interface Wider { fn name() -> string fn other() -> int }
struct User {
    fn name(self) -> string { return "user" }
    fn other(self) -> int { return 1 }
}
let named = User {} as Named
let narrow = (User {} as Wider) as Named
"#;
    check(source)
        .expect("structural concrete/interface and interface/interface casts should check");
}

#[test]
fn collecting_checker_snapshots_interfaces_on_errors() {
    let source = "interface I { fn value() -> int }\nfn consume(value: I) { value.missing() }\n";
    let program = parse_source(source).expect("source should parse");
    let (checked, diagnostics) = lirac::checker::check_collecting(&program);
    let checked = checked.expect("collecting checker must retain semantic tables");
    assert!(!diagnostics.is_empty());
    assert!(checked.sema.type_members.contains_key("I"));
    assert!(matches!(
        checked.sema.type_members["I"].methods[0].ty,
        Type::Function { .. }
    ));
}

#[test]
fn void_implementation_cannot_satisfy_value_returning_interface_method() {
    let source = r#"
interface PushValue { fn push(value: int) -> any }
let values = [1]
let invalid: PushValue = values
"#;
    let diagnostics = analyze(source).expect("source should parse").diagnostics;
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("method 'push' has an incompatible signature")
    }));
}
