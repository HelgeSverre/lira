use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn temp_dir() -> PathBuf {
    let id = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("lira-disasm-test-{}-{}", std::process::id(), id));
    fs::create_dir(&path).expect("create isolated test directory");
    path
}

fn compile_source(path: &Path) -> (PathBuf, Vec<u8>) {
    let source = fs::read_to_string(path).expect("read source fixture");
    let source_name = path.to_string_lossy();
    let bytecode = lirac::compile_with_imports(&source_name, &source).expect("compile source");

    let dir = temp_dir();
    let bytecode_path = dir.join("program.lic");
    fs::write(&bytecode_path, &bytecode).expect("write bytecode fixture");
    (bytecode_path, bytecode)
}

fn disassemble(path: &Path) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_lira"))
        .args(["disasm", path.to_str().expect("bytecode path is UTF-8")])
        .output()
        .expect("run lira disasm");
    assert!(
        output.status.success(),
        "disassembler failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("disassembler output is UTF-8")
}

fn disassemble_error(path: &Path) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_lira"))
        .args(["disasm", path.to_str().expect("bytecode path is UTF-8")])
        .output()
        .expect("run lira disasm");
    assert!(!output.status.success());
    String::from_utf8(output.stderr).expect("disassembler error is UTF-8")
}

fn bytecode_with_code(code: &[u8]) -> Vec<u8> {
    let mut bytecode = Vec::with_capacity(28 + code.len());
    bytecode.extend_from_slice(&lira_core::BYTECODE_MAGIC.to_le_bytes());
    bytecode.extend_from_slice(&lira_core::BYTECODE_VERSION.to_le_bytes());
    bytecode.extend_from_slice(&0_u32.to_le_bytes());
    bytecode.extend_from_slice(&0_u32.to_le_bytes());
    bytecode.extend_from_slice(&0_u32.to_le_bytes());
    bytecode.extend_from_slice(&0_u32.to_le_bytes());
    bytecode.extend_from_slice(&(code.len() as u32).to_le_bytes());
    bytecode.extend_from_slice(code);
    bytecode
}

#[test]
fn disassembly_round_trips_real_calls_constants_and_closures() {
    let (bytecode_path, bytecode) = compile_source(&repo_path("examples/tail_call_regressions.li"));
    let text = disassemble(&bytecode_path);

    // The fixture deliberately creates more than 100 constants and exercises
    // both ordinary and tail calls, closure captures, u16 locals, and value
    // copies. These assertions also catch operand-width desynchronization.
    assert!(text.contains("LoadConst 102"));
    assert!(text.contains("StoreLocal 95"));
    assert!(text.contains("MakeClosure fn:"));
    assert!(text.contains("Call (0)"));
    assert!(text.contains("TailCall (2)"));
    assert!(text.contains("CopyValue"));

    let (exit_code, output) = liravm::run_with_capture(&bytecode).expect("execute bytecode");
    assert_eq!(exit_code, 0);
    assert_eq!(output, ["2000", "15", "3628800", "125", "2.5"]);

    cleanup(&bytecode_path);
}

#[test]
fn disassembly_decodes_struct_class_and_select_instructions() {
    let struct_path = repo_path("examples/impl_block.li");
    let (struct_bytecode_path, struct_bytecode) = compile_source(&struct_path);
    let struct_text = disassemble(&struct_bytecode_path);
    assert!(struct_text.contains("NewStruct"));
    assert!(struct_text.contains("GetField"));
    assert!(struct_text.contains("SetField"));
    let (exit_code, output) =
        liravm::run_with_capture(&struct_bytecode).expect("execute struct bytecode");
    assert_eq!(exit_code, 0);
    assert_eq!(output, ["0", "1", "2"]);
    cleanup(&struct_bytecode_path);

    let class_path = repo_path("examples/class_this.li");
    let (class_bytecode_path, class_bytecode) = compile_source(&class_path);
    let class_text = disassemble(&class_bytecode_path);
    assert!(class_text.contains("NewObject"));
    let (exit_code, output) =
        liravm::run_with_capture(&class_bytecode).expect("execute class bytecode");
    assert_eq!(exit_code, 0);
    assert_eq!(
        output,
        ["name: Rex", "bark: Rex says Woof", "this-check: Rex"]
    );
    cleanup(&class_bytecode_path);

    let select_path = repo_path("tests/samples/producer-consumer.li");
    let (select_bytecode_path, select_bytecode) = compile_source(&select_path);
    let select_text = disassemble(&select_bytecode_path);
    assert!(select_text.contains("Select arms:"));
    assert!(select_text.contains("recv->"));
    assert!(select_text.contains("send->"));
    let (exit_code, output) =
        liravm::run_with_capture(&select_bytecode).expect("execute select bytecode");
    assert_eq!(exit_code, 0);
    assert!(output
        .iter()
        .any(|line| line == "=== Producer-Consumer Demo ==="));
    assert!(output.iter().any(|line| line == "=== Demo Complete ==="));
    cleanup(&select_bytecode_path);
}

#[test]
fn disassembly_names_tuple_runtime_type_ids() {
    let bytecode =
        lirac::compile("println((1, 2) is (int, int))").expect("compile tuple type check");
    let dir = temp_dir();
    let bytecode_path = dir.join("tuple-type.lic");
    fs::write(&bytecode_path, &bytecode).expect("write tuple bytecode fixture");

    let text = disassemble(&bytecode_path);
    assert!(
        text.contains("TypeIs tuple"),
        "unexpected disassembly:\n{text}"
    );
    let (status, output) = liravm::run_with_capture(&bytecode).expect("execute tuple type check");
    assert_eq!(status, 0);
    assert_eq!(output, ["true"]);

    cleanup(&bytecode_path);
}

#[test]
fn disassembly_decodes_interface_box_call_and_is_without_desynchronizing() {
    let source = r#"
interface Named { fn name() -> string }
struct User {
    fn name(self) -> string { return "user" }
}
let named: Named = User {}
println(named.name())
println(named is Named)
"#;
    let bytecode = lirac::compile(source).expect("interface source should compile");
    let dir = temp_dir();
    let bytecode_path = dir.join("interface.lic");
    fs::write(&bytecode_path, &bytecode).expect("write interface bytecode fixture");

    let text = disassemble(&bytecode_path);
    assert!(
        text.contains("InterfaceBox"),
        "unexpected disassembly:\n{text}"
    );
    assert!(
        text.contains("InterfaceCall"),
        "unexpected disassembly:\n{text}"
    );
    assert!(
        text.contains("InterfaceIs"),
        "unexpected disassembly:\n{text}"
    );
    assert!(text.contains("Println"), "unexpected disassembly:\n{text}");
    assert!(
        !text.contains("Unknown opcode"),
        "unexpected disassembly:\n{text}"
    );

    let (exit_code, output) =
        liravm::run_with_capture(&bytecode).expect("execute interface bytecode");
    assert_eq!(exit_code, 0);
    assert_eq!(output, ["user", "true"]);

    cleanup(&bytecode_path);
}

#[test]
fn disassembly_reports_truncated_interface_witness() {
    let code = [lira_core::opcode::Opcode::InterfaceBox as u8, 0, 1, 0, 0];
    let dir = temp_dir();
    let path = dir.join("truncated-interface.lic");
    fs::write(&path, bytecode_with_code(&code)).expect("write malformed bytecode");

    let error = disassemble_error(&path);
    assert!(error.contains("Unexpected end of bytecode"), "{error}");
    assert!(error.contains("InterfaceBox witness kind"), "{error}");

    cleanup(&path);
}

#[test]
fn disassembly_reports_invalid_interface_witness_kind() {
    let code = [
        lira_core::opcode::Opcode::InterfaceIs as u8,
        u8::MAX,
        1,
        0,
        0,
        9,
    ];
    let dir = temp_dir();
    let path = dir.join("invalid-interface-kind.lic");
    fs::write(&path, bytecode_with_code(&code)).expect("write malformed bytecode");

    let error = disassemble_error(&path);
    assert!(
        error.contains("Invalid InterfaceIs witness kind 9"),
        "{error}"
    );
    assert!(error.contains("method 0"), "{error}");

    cleanup(&path);
}

#[test]
fn disassembly_reports_truncated_operands_without_panicking() {
    let dir = temp_dir();
    let path = dir.join("truncated.lic");
    let mut bytecode = Vec::new();
    bytecode.extend_from_slice(&0x4C494243_u32.to_le_bytes());
    bytecode.extend_from_slice(&1_u32.to_le_bytes());
    bytecode.extend_from_slice(&0_u32.to_le_bytes());
    bytecode.extend_from_slice(&0_u32.to_le_bytes());
    bytecode.extend_from_slice(&0_u32.to_le_bytes());
    bytecode.extend_from_slice(&0_u32.to_le_bytes());
    bytecode.extend_from_slice(&1_u32.to_le_bytes());
    bytecode.push(lira_core::opcode::Opcode::LoadConst as u8);
    fs::write(&path, bytecode).expect("write malformed bytecode");

    let output = Command::new(env!("CARGO_BIN_EXE_lira"))
        .args(["disasm", path.to_str().expect("bytecode path is UTF-8")])
        .output()
        .expect("run lira disasm");
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("Unexpected end of bytecode"), "{error}");
    assert!(error.contains("u16 operand"), "{error}");

    cleanup(&path);
}

fn cleanup(bytecode_path: &Path) {
    let dir = bytecode_path.parent().expect("test bytecode has parent");
    fs::remove_file(bytecode_path).expect("remove test bytecode");
    fs::remove_dir(dir).expect("remove empty test directory");
}
