//! End-to-end binding OR-pattern coverage for the checker and bytecode VM.

fn run_vm(source: &str) -> Result<String, String> {
    let bytecode = lirac::compile(source)?;
    liravm::bytecode::load(&bytecode)
        .map_err(|error| format!("load compiled bytecode: {error}"))?;
    let (status, lines) = liravm::run_with_capture(&bytecode)?;
    if status != 0 {
        return Err(format!("VM exited with status {status}"));
    }
    Ok(lines.join("\n"))
}

fn diagnostics(source: &str) -> Vec<lirac::Diagnostic> {
    lirac::analyze(source)
        .expect("OR-pattern source should lex and parse")
        .diagnostics
}

#[test]
fn matching_alternative_binds_its_own_payload_on_the_vm() {
    let source = r#"
        enum Choice { Left(int), Right(int), Pair(int, int) }

        fn value(choice: Choice) -> int {
            return match choice {
                Choice::Left(x) | Choice::Right(x) => x,
                Choice::Pair(x, 0 | 1) => x
            }
        }

        println(value(Choice::Left(11)))
        println(value(Choice::Right(22)))
        println(value(Choice::Pair(33, 0)))
        println(value(Choice::Pair(44, 1)))
    "#;

    assert_eq!(
        run_vm(source).expect("binding OR-pattern VM run"),
        "11\n22\n33\n44"
    );
}

#[test]
fn top_level_or_pattern_matches_each_alternative_and_falls_through() {
    let source = r#"
        enum Color { Red, Green, Blue }

        fn classify(color: Color) -> string {
            return match color {
                Color::Red | Color::Green => "warm",
                _ => "other"
            }
        }

        println(classify(Color::Red))
        println(classify(Color::Green))
        println(classify(Color::Blue))
    "#;

    assert_eq!(
        run_vm(source).expect("top-level OR-pattern VM run"),
        "warm\nwarm\nother"
    );
}

#[test]
fn or_pattern_alternatives_must_bind_the_same_names() {
    let different_names = "enum Choice { Left(int), Right(int) }\nfn bad(value: Choice) -> int {\n    return match value {\n        Choice::Left(x) | Choice::Right(y) => x\n    }\n}\n";
    let errors = diagnostics(different_names);
    let mismatch = errors
        .iter()
        .find(|error| error.message.contains("must bind the same variables"))
        .expect("different OR-pattern binding names must be diagnosed");
    assert_eq!(mismatch.line, 4);
    assert!(mismatch.column > 0);

    let missing_name = "enum Choice { Left(int), Right(int) }\nfn bad(value: Choice) -> int {\n    return match value {\n        Choice::Left(x) | Choice::Right(_) => x\n    }\n}\n";
    assert!(diagnostics(missing_name)
        .iter()
        .any(|error| error.message.contains("must bind the same variables")));
}

#[test]
fn or_pattern_alternatives_must_bind_the_same_type() {
    let source = "enum Mixed { Number(int), Text(string) }\nfn bad(value: Mixed) -> int {\n    return match value {\n        Mixed::Number(x) | Mixed::Text(x) => 0\n    }\n}\n";
    let errors = diagnostics(source);
    let mismatch = errors
        .iter()
        .find(|error| error.message.contains("incompatible types") && error.message.contains("'x'"))
        .expect("incompatible OR-pattern binding types must be diagnosed");
    assert_eq!(mismatch.line, 4);
    assert!(mismatch.column > 0);
}
