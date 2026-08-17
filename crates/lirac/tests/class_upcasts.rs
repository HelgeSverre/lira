//! Real-source checker and bytecode coverage for nominal class subtyping.

use lirac::{analyze, check, compile_with_imports};

const SOURCE: &str = r#"
class Animal {
    fn speak(self) -> string { return "Animal" }
}

class Dog extends Animal {
    override fn speak(self) -> string { return "Dog" }
}

class Puppy extends Dog {
    override fn speak(self) -> string { return "Puppy" }
}

fn accept(animal: Animal) -> string { return animal.speak() }
fn return_animal() -> Animal { return Puppy {} }

fn main() {
    let dog = Dog {}
    let animal: Animal = dog
    let animals: [Animal] = [dog]
    let maybe_animal: Animal? = dog
    println(accept(dog))
    println(animal.speak())
    println(animals[0].speak())
    println(return_animal().speak())
}

main()
"#;

#[test]
fn class_upcasts_check_in_nested_contexts_and_dispatch_in_vm() {
    check(SOURCE).expect("child classes should be accepted where parents are expected");

    let bytecode = compile_with_imports("class_upcasts.li", SOURCE).expect("source compiles");
    let (status, output) = liravm::run_with_capture(&bytecode).expect("VM executes source");
    assert_eq!(status, 0);
    assert_eq!(output, ["Dog", "Dog", "Dog", "Puppy"]);
}

#[test]
fn sibling_and_downcasts_are_rejected_with_argument_and_initializer_spans() {
    let source = r#"
class Animal {}
class Dog extends Animal {}
class Cat extends Animal {}

fn take_dog(dog: Dog) {}
fn take_animal(animal: Animal) {}

let animal: Animal = Dog {}
let dog: Dog = animal
take_dog(animal)
take_dog(Cat {})
take_animal(dog)
"#;
    let diagnostics = analyze(source).expect("source parses").diagnostics;

    let expected_mismatches = [
        "Type mismatch: expected 'Dog', got 'Animal'",
        "Argument type mismatch: expected 'Dog', got 'Animal'",
        "Argument type mismatch: expected 'Dog', got 'Cat'",
    ];
    for expected in expected_mismatches {
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message == expected),
            "missing exact diagnostic {expected:?}: {diagnostics:?}"
        );
    }
    let mismatch_count = diagnostics
        .iter()
        .filter(|diagnostic| expected_mismatches.contains(&diagnostic.message.as_str()))
        .count();
    assert_eq!(
        mismatch_count, 3,
        "unexpected mismatch diagnostics: {diagnostics:?}"
    );
    // The valid child-to-parent declaration and call must not be diagnosed.
    assert!(!diagnostics.iter().any(|diagnostic| {
        diagnostic.message == "Type mismatch: expected 'Animal', got 'Dog'"
            || diagnostic.message == "Argument type mismatch: expected 'Animal', got 'Dog'"
    }));
    assert!(diagnostics
        .iter()
        .filter(|diagnostic| expected_mismatches.contains(&diagnostic.message.as_str()))
        .all(|diagnostic| diagnostic.line >= 8));
}

#[test]
fn mutable_containers_are_invariant_but_safe_array_literals_and_pushes_are_allowed() {
    let valid = r#"
class Animal {}
class Dog extends Animal {}

let animals: [Animal] = [Dog {}]
animals.push(Dog {})
let nested_animals: [[Animal]] = [[Dog {}]]
let pair: (Animal, int) = (Dog {}, 1)
"#;
    check(valid).expect("fresh literals and subtype element insertion are safe");

    let invalid = r#"
class Animal {}
class Dog extends Animal {}

let dogs: [Dog] = [Dog {}]
let animals: [Animal] = dogs
let dog_matrix: [[Dog]] = [[Dog {}]]
let animal_matrix: [[Animal]] = dog_matrix
let dog_channels: Channel<Dog> = chan(1)
let animal_channels: Channel<Animal> = dog_channels
let dog_map: Map<string, Dog> = { "dog": Dog {} }
let animal_map: Map<string, Animal> = dog_map
"#;
    let diagnostics = analyze(invalid).expect("source parses").diagnostics;
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message == "Type mismatch: expected '[Animal]', got '[Dog]'"));
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message
            == "Type mismatch: expected '[[Animal]]', got '[[Dog]]'"));
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.message
        == "Type mismatch: expected 'Channel<Animal>', got 'Channel<Dog>'"));
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.message
        == "Type mismatch: expected 'Map<string, Animal>', got 'Map<string, Dog>'"));
}

#[test]
fn class_overrides_require_compatible_inputs_and_covariant_returns() {
    let valid = r#"
class Animal {}
class Dog extends Animal {}
class Parent {
    fn make(value: Animal) -> Animal { return value }
}
class Child extends Parent {
    override fn make(value: Animal) -> Dog { return Dog {} }
}
"#;
    check(valid).expect("same parameters and covariant class return are valid");

    let invalid = r#"
class Animal {}
class Dog extends Animal {}
class Parent {
    fn make(value: Animal) -> Animal { return value }
}
class BadParameter extends Parent {
    override fn make(value: Dog) -> Animal { return Animal {} }
}
class BadReturn extends Parent {
    override fn make(value: Animal) -> string { return "bad" }
}
class BadArity extends Parent {
    override fn make(value: Animal, extra: int) -> Animal { return value }
}
class BadReceiver extends Parent {
    override fn make(self, value: Animal) -> Animal { return value }
}
"#;
    let diagnostics = analyze(invalid).expect("source parses").diagnostics;
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.message
        == "Method 'make' override parameter 1 type mismatch: expected 'Animal', got 'Dog'"));
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.message
        == "Method 'make' override return type mismatch: expected 'Animal', got 'string'"));
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.message
        == "Method 'make' override has incompatible arity: expected 1 parameters, got 2"));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.message
        == "Method 'make' override receiver mismatch: expected static method, got instance method"
    }));
}

#[test]
fn static_class_methods_use_their_real_receiver_metadata_for_arity() {
    let valid = r#"
class Factory {
    value: int
    fn new(value: int) -> Factory { return Factory { value: value } }
    fn get(self) -> int { return self.value }
}
let factory = Factory.new(7)
println(factory.get())
"#;
    check(valid).expect("static method should consume its explicit argument");

    let invalid = r#"
class Factory {
    fn new(value: int) -> Factory { return Factory {} }
}
let factory = Factory.new()
"#;
    let diagnostics = analyze(invalid).expect("source parses").diagnostics;
    assert!(diagnostics.iter().any(|diagnostic| diagnostic
        .message
        .contains("Expected at least 1 arguments, got 0")));
}

#[test]
fn inheritance_cycles_are_reported_once_and_deterministically() {
    let source = r#"
class A extends B {}
class B extends A {}
"#;
    let diagnostics = analyze(source).expect("source parses").diagnostics;
    let cycles: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic
                .message
                .starts_with("Inheritance cycle detected:")
        })
        .collect();
    assert_eq!(
        cycles.len(),
        1,
        "unexpected cycle diagnostics: {diagnostics:?}"
    );
    assert_eq!(cycles[0].message, "Inheritance cycle detected: A -> B -> A");
}

#[test]
fn method_receiver_kind_must_match_the_access_form() {
    let valid = r#"
class Factory {
    fn new(value: int) -> Factory { return Factory {} }
    fn get(self) -> int { return 1 }
}
let factory = Factory {}
factory.get()
Factory.new(1)
"#;
    check(valid).expect("instance and static receiver forms are valid");

    let invalid = r#"
class Factory {
    fn new(value: int) -> Factory { return Factory {} }
    fn get(self) -> int { return 1 }
}
let factory = Factory {}
factory.new(1)
Factory.get()
"#;
    let diagnostics = analyze(invalid).expect("source parses").diagnostics;
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.message == "Cannot access static method 'new' through an instance of 'Factory'"
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.message == "Cannot access instance method 'get' through type 'Factory'"
    }));
}

#[test]
fn fresh_nested_array_literals_use_declared_aggregate_context() {
    let valid = r#"
class Animal {}
class Dog extends Animal {}
struct Kennel { animals: [Animal] }
let kennel = Kennel { animals: [Dog {}] }
let tuple: ([Animal], int) = ([Dog {}], 1)
let optional: [Animal]? = [Dog {}]
"#;
    check(valid).expect("fresh aggregate literals may narrow class elements");

    let invalid = r#"
class Animal {}
class Dog extends Animal {}
let dogs: [Dog] = [Dog {}]
let kennel: [Animal] = dogs
"#;
    let diagnostics = analyze(invalid).expect("source parses").diagnostics;
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.message == "Type mismatch: expected '[Animal]', got '[Dog]'"
    }));
}

#[test]
fn overrides_preserve_each_parent_default_and_return_contract() {
    let invalid = r#"
class Animal {}
class Parent {
    fn make(a: int, b: int = 2, c: int = 3) -> Animal { return Animal {} }
}
class Shifted extends Parent {
    override fn make(a: int = 1, b: int, c: int = 3) -> Animal { return Animal {} }
}
class AnyReturn extends Parent {
    override fn make(a: int, b: int = 2, c: int = 3) -> any { return 1 }
}
class NumericReturn extends Parent {
    override fn make(a: int, b: int = 2, c: int = 3) -> float { return 1.0 }
}
"#;
    let diagnostics = analyze(invalid).expect("source parses").diagnostics;
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.message
            == "Method 'make' override parameter 2 default mismatch: parent parameter has a default, child parameter is required"
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.message
            == "Method 'make' override return type mismatch: expected 'Animal', got 'any'"
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.message
            == "Method 'make' override return type mismatch: expected 'Animal', got 'float'"
    }));
}

#[test]
fn inferred_class_array_element_type_is_independent_of_source_order() {
    let valid = r#"
class Animal {}
class Dog extends Animal {}
let first = [Dog {}, Animal {}]
let second = [Animal {}, Dog {}]
let first_as_animals: [Animal] = first
let second_as_animals: [Animal] = second
"#;
    check(valid).expect("both source orders infer Animal as the common class type");

    let invalid = r#"
class Dog {}
class Cat {}
let unrelated = [Dog {}, Cat {}]
"#;
    let diagnostics = analyze(invalid).expect("source parses").diagnostics;
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.message == "Array element type mismatch: expected 'Dog', got 'Cat'"
    }));
}

#[test]
fn mutable_storage_requires_exact_existing_types_and_concrete_fresh_values() {
    let valid = r#"
const ints: [int] = []
ints.push(1)
let number: any = 1
let text: any = "text"
let explicit_any: [any] = [number, text]
"#;
    check(valid).expect("annotated fresh storage and explicit any storage are valid");

    let invalid = r#"
let anys: [any] = [1]
let ints_from_anys: [int] = anys
let ints: [int] = [1]
let floats: [float] = ints
let any_value: any = "text"
let fresh_ints: [int] = [any_value]
let any_channel: Channel<any> = chan(1)
let int_channel: Channel<int> = any_channel
let any_map: Map<string, any> = { "value": 1 }
let int_map: Map<string, int> = any_map
"#;
    let diagnostics = analyze(invalid).expect("source parses").diagnostics;
    for expected in [
        "Type mismatch: expected '[int]', got '[any]'",
        "Type mismatch: expected '[float]', got '[int]'",
        "Type mismatch: expected 'Channel<int>', got 'Channel<any>'",
        "Type mismatch: expected 'Map<string, int>', got 'Map<string, any>'",
    ] {
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message == expected),
            "missing {expected:?}: {diagnostics:?}"
        );
    }
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.message == "Type mismatch: expected '[int]', got '[any]'" && diagnostic.line == 7
    }));
}

#[test]
fn unresolved_arrays_cannot_be_aliased_before_inference() {
    let source = r#"
let values = []
let value_alias = values
"#;
    let diagnostics = analyze(source).expect("source parses").diagnostics;
    let alias_errors: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.message == "Cannot alias mutable storage before its element type is inferred"
        })
        .collect();
    assert_eq!(
        alias_errors.len(),
        1,
        "unexpected diagnostics: {diagnostics:?}"
    );
    assert_eq!(alias_errors[0].line, 3);
}

#[test]
fn function_parameters_are_checked_contravariantly() {
    let valid = r#"
class Animal {}
class Dog extends Animal {}
fn accepts_animal(value: Animal) -> int { return 1 }
let accepts_dog: fn(Dog) -> int = accepts_animal
"#;
    check(valid).expect("a broader-input function may satisfy a narrower-input callback");

    let invalid = r#"
class Animal {}
class Dog extends Animal {}
fn accepts_dog(value: Dog) -> int { return 1 }
let accepts_animal: fn(Animal) -> int = accepts_dog
"#;
    let diagnostics = analyze(invalid).expect("source parses").diagnostics;
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.message == "Type mismatch: expected 'fn(Animal) -> int', got 'fn(Dog) -> int'"
    }));
}

#[test]
fn classes_reject_duplicate_and_inherited_field_names() {
    let source = r#"
class Parent { value: int }
class Child extends Parent { value: string }
class Duplicate {
    item: int
    item: string
}
"#;
    let diagnostics = analyze(source).expect("source parses").diagnostics;
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.message == "Field 'value' in class 'Child' conflicts with an inherited field"
    }));
    assert!(diagnostics
        .iter()
        .any(|diagnostic| { diagnostic.message == "Duplicate field 'item' in class 'Duplicate'" }));
}
