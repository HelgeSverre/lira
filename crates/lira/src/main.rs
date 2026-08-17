//! Lira CLI - Unified interface for Lira programming language
//!
//! Commands:
//!   lira run <file.li>                 Compile and execute Lira source
//!   lira build <file.li> [-o <out>]    Compile to a native executable
//!   lira jit <file.li>                 Compile to native code in an isolated worker
//!   lira compile <file.li> [-o <out>]  Compile to bytecode
//!   lira check <file.li>               Type check without compiling
//!   lira ast <file.li>                 Dump parsed AST as JSON
//!   lira disasm <file.lic>             Disassemble bytecode
//!   lira --help                        Show help
//!   lira --version                     Show version

use lira_core::opcode::Opcode;
use std::env;
use std::fs;
use std::io::Read;
use std::process;

const MAX_INTERFACE_METHODS: usize = u8::MAX as usize;

fn main() {
    let args: Vec<String> = env::args().collect();

    // This private protocol is used only by `lira jit`'s containment runner.
    // Dispatch it before normal argument/help handling so it never appears in
    // the public command usage.
    if args.get(1).map(String::as_str) == Some("__jit-worker") {
        jit_worker_command(&args);
    }

    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let result = match args[1].as_str() {
        "run" => {
            if args.len() < 3 {
                eprintln!("Error: Missing input file");
                eprintln!("Usage: lira run <file.li>");
                process::exit(1);
            }
            run_command(&args[2])
        }
        "build" => {
            if args.len() < 3 {
                eprintln!("Error: Missing input file");
                eprintln!("Usage: lira build <file.li> [-o <output>]");
                process::exit(1);
            }
            let output = parse_output_arg_with_default(&args, &native_output_name(&args[2]));
            build_command(&args[2], &output).map(|_| 0)
        }
        "jit" => {
            if args.len() < 3 {
                eprintln!("Error: Missing input file");
                eprintln!("Usage: lira jit <file.li>");
                process::exit(1);
            }
            jit_command(&args[2])
        }
        "compile" => {
            if args.len() < 3 {
                eprintln!("Error: Missing input file");
                eprintln!("Usage: lira compile <file.li> [-o <output.lic>]");
                process::exit(1);
            }
            let output = parse_output_arg(&args, &args[2]);
            compile_command(&args[2], &output).map(|_| 0)
        }
        "check" => {
            if args.len() < 3 {
                eprintln!("Error: Missing input file");
                eprintln!("Usage: lira check <file.li>");
                process::exit(1);
            }
            check_command(&args[2]).map(|_| 0)
        }
        "ast" => {
            if args.len() < 3 {
                eprintln!("Error: Missing input file");
                eprintln!("Usage: lira ast <file.li>");
                process::exit(1);
            }
            ast_command(&args[2]).map(|_| 0)
        }
        "disasm" => {
            if args.len() < 3 {
                eprintln!("Error: Missing input file");
                eprintln!("Usage: lira disasm <file.lic>");
                process::exit(1);
            }
            disasm_command(&args[2]).map(|_| 0)
        }
        "--help" | "-h" | "help" => {
            print_usage();
            process::exit(0);
        }
        "--version" | "-V" | "version" => {
            println!("lira {}", env!("CARGO_PKG_VERSION"));
            process::exit(0);
        }
        cmd => {
            eprintln!("Unknown command: {}", cmd);
            eprintln!();
            print_usage();
            process::exit(1);
        }
    };

    match result {
        Ok(exit_code) => process::exit(exit_code),
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    }
}

/// Compile and execute a Lira source file
fn run_command(file: &str) -> Result<i32, String> {
    // Read source
    let source = fs::read_to_string(file).map_err(|e| format!("Failed to read {}: {}", file, e))?;

    // Compile in-memory with import resolution
    let bytecode = lirac::compile_with_imports(file, &source)?;

    // Execute and return exit code
    liravm::run(&bytecode)
}

/// Compile a Lira source file to a standalone native executable
fn build_command(input: &str, output: &str) -> Result<(), String> {
    let source =
        fs::read_to_string(input).map_err(|e| format!("Failed to read {}: {}", input, e))?;
    lira_codegen::build_native(input, &source, std::path::Path::new(output))?;
    println!("Compiled {} -> {}", input, output);
    Ok(())
}

/// Compile a Lira source file to native code in an isolated worker process.
fn jit_command(file: &str) -> Result<i32, String> {
    let source = read_isolated_jit_source(file)?;
    let worker = env::current_exe()
        .map_err(|e| format!("Failed to locate lira executable for JIT worker: {e}"))?;
    lira_codegen::jit_run_isolated(&worker, file, &source)
}

fn read_isolated_jit_source(file: &str) -> Result<String, String> {
    let input = fs::File::open(file).map_err(|e| format!("Failed to read {file}: {e}"))?;
    let mut bytes = Vec::new();
    input
        .take((lira_codegen::ISOLATED_JIT_MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Failed to read {file}: {e}"))?;
    if bytes.len() > lira_codegen::ISOLATED_JIT_MAX_SOURCE_BYTES {
        return Err(format!(
            "JIT source limit exceeded: {} is larger than {} bytes",
            file,
            lira_codegen::ISOLATED_JIT_MAX_SOURCE_BYTES
        ));
    }
    String::from_utf8(bytes).map_err(|_| format!("Failed to read {file}: source is not UTF-8"))
}

/// Run one JIT module inside the private worker protocol.
///
/// The parent gives us private source and result paths. Keep
/// generated stdout/stderr untouched: the parent captures and relays those
/// streams, while this file carries only the machine-readable status.
fn jit_worker_command(args: &[String]) -> ! {
    if args.len() != 5 {
        eprintln!("JIT worker protocol error");
        process::exit(2);
    }
    let source_file = &args[2];
    let source_path = &args[3];
    let result_path = &args[4];
    let outcome = read_isolated_jit_source(source_path)
        .map_err(|e| format!("worker could not read source: {e}"))
        .and_then(|source| lira_codegen::jit_run_in_process(source_file, &source));
    let result = match outcome {
        Ok(status) => format!("ok:{status}\n"),
        Err(error) => format!("err:{error}\n"),
    };
    if let Err(error) = fs::write(result_path, result) {
        eprintln!("JIT worker could not write result: {error}");
        process::exit(2);
    }
    process::exit(0);
}

/// Compile a Lira source file to bytecode
fn compile_command(input: &str, output: &str) -> Result<(), String> {
    lirac::compile_file(input, output)?;
    println!("Compiled {} -> {}", input, output);
    Ok(())
}

/// Type check a Lira source file
fn check_command(file: &str) -> Result<(), String> {
    lirac::check_file(file)?;
    println!("No errors found in {}", file);
    Ok(())
}

/// Dump the AST of a Lira source file as JSON
fn ast_command(file: &str) -> Result<(), String> {
    let json = lirac::parse_file_json(file)?;
    println!("{}", json);
    Ok(())
}

/// Disassemble a bytecode file
fn disasm_command(file: &str) -> Result<(), String> {
    let bytecode = fs::read(file).map_err(|e| format!("Failed to read {}: {}", file, e))?;
    let output = disassemble(&bytecode)?;
    print!("{}", output);
    Ok(())
}

/// Disassemble bytecode into human-readable format
fn disassemble(bytecode: &[u8]) -> Result<String, String> {
    use lira_core::{BYTECODE_MAGIC, BYTECODE_VERSION};

    if bytecode.len() < 24 {
        return Err(format!(
            "Bytecode too short: expected at least 24 bytes, got {}",
            bytecode.len()
        ));
    }

    struct Reader<'a> {
        bytes: &'a [u8],
        pos: usize,
        end: usize,
    }

    impl<'a> Reader<'a> {
        fn new(bytes: &'a [u8]) -> Self {
            Self {
                bytes,
                pos: 0,
                end: bytes.len(),
            }
        }

        fn position(&self) -> usize {
            self.pos
        }

        fn set_end(&mut self, end: usize) -> Result<(), String> {
            if end < self.pos || end > self.bytes.len() {
                return Err(format!(
                    "Bytecode section exceeds input at offset {}: end {}",
                    self.pos, end
                ));
            }
            self.end = end;
            Ok(())
        }

        fn take(&mut self, count: usize, what: &str) -> Result<&'a [u8], String> {
            let end = self
                .pos
                .checked_add(count)
                .ok_or_else(|| format!("{} length overflows at offset {}", what, self.pos))?;
            if end > self.end {
                return Err(format!(
                    "Unexpected end of bytecode while reading {} at offset {} (needed {} bytes)",
                    what, self.pos, count
                ));
            }
            let result = &self.bytes[self.pos..end];
            self.pos = end;
            Ok(result)
        }

        fn u8(&mut self, what: &str) -> Result<u8, String> {
            Ok(self.take(1, what)?[0])
        }

        fn u16(&mut self, what: &str) -> Result<u16, String> {
            let bytes = self.take(2, what)?;
            Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
        }

        fn i16(&mut self, what: &str) -> Result<i16, String> {
            let bytes = self.take(2, what)?;
            Ok(i16::from_le_bytes([bytes[0], bytes[1]]))
        }

        fn u32(&mut self, what: &str) -> Result<u32, String> {
            let bytes = self.take(4, what)?;
            Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        }

        fn i64(&mut self, what: &str) -> Result<i64, String> {
            let bytes = self.take(8, what)?;
            let bytes: [u8; 8] = bytes
                .try_into()
                .map_err(|_| format!("Invalid {} width", what))?;
            Ok(i64::from_le_bytes(bytes))
        }

        fn f64(&mut self, what: &str) -> Result<f64, String> {
            let bytes = self.take(8, what)?;
            let bytes: [u8; 8] = bytes
                .try_into()
                .map_err(|_| format!("Invalid {} width", what))?;
            Ok(f64::from_le_bytes(bytes))
        }

        fn interface_witnesses(
            &mut self,
            method_count: usize,
            opcode_name: &str,
            has_function_offset: bool,
            code_start: usize,
        ) -> Result<Vec<String>, String> {
            if method_count > MAX_INTERFACE_METHODS {
                return Err(format!(
                    "{} method count {} exceeds maximum {}",
                    opcode_name, method_count, MAX_INTERFACE_METHODS
                ));
            }

            let mut witnesses = Vec::with_capacity(method_count);
            for method_index in 0..method_count {
                let name_const = self.u16(&format!("{} method name constant", opcode_name))?;
                let kind_offset = self.position();
                let kind = self.u8(&format!("{} witness kind", opcode_name))?;
                let kind_text = match kind {
                    0 => "existing".to_string(),
                    1 if has_function_offset => {
                        let function_offset =
                            self.u16(&format!("{} function offset", opcode_name))?;
                        format!("function@{}", function_offset)
                    }
                    1 => "function".to_string(),
                    2 => {
                        let intrinsic = self.u8(&format!("{} intrinsic", opcode_name))?;
                        format!("intrinsic#{}", intrinsic)
                    }
                    invalid => {
                        let code_offset = kind_offset.checked_sub(code_start).ok_or_else(|| {
                            format!("{} witness kind precedes code section", opcode_name)
                        })?;
                        return Err(format!(
                            "Invalid {} witness kind {} at code offset {} (method {})",
                            opcode_name, invalid, code_offset, method_index
                        ));
                    }
                };
                witnesses.push(format!("name:{} {}", name_const, kind_text));
            }
            Ok(witnesses)
        }
    }

    let mut output = String::new();
    let mut reader = Reader::new(bytecode);

    // Parse header
    let magic = reader.u32("header magic")?;
    let version = reader.u32("header version")?;
    let flags = reader.u32("header flags")?;
    let entry_point = reader.u32("header entry point")?;
    let constant_count = reader.u32("header constant count")?;
    let function_count = reader.u32("header function count")?;

    output.push_str("=== BYTECODE HEADER ===\n");
    output.push_str(&format!("Magic:          0x{:08X}", magic));
    if magic == BYTECODE_MAGIC {
        output.push_str(" (valid)\n");
    } else {
        output.push_str(" (INVALID!)\n");
    }
    output.push_str(&format!("Version:        {}", version));
    if version == BYTECODE_VERSION {
        output.push_str(" (current)\n");
    } else {
        output.push_str(" (mismatch!)\n");
    }
    output.push_str(&format!("Flags:          0x{:08X}\n", flags));
    output.push_str(&format!("Entry point:    {}\n", entry_point));
    output.push_str(&format!("Constants:      {}\n", constant_count));
    output.push_str(&format!("Functions:      {}\n", function_count));
    output.push('\n');

    // Parse constants
    output.push_str("=== CONSTANT POOL ===\n");
    for i in 0..constant_count {
        let tag_offset = reader.position();
        let tag = reader.u8("constant tag")?;
        let value_str = match tag {
            0x00 => "null".to_string(),
            0x01 => {
                let b = reader.u8("boolean constant")?;
                format!("bool: {}", b != 0)
            }
            0x02 => {
                let n = reader.i64("integer constant")?;
                format!("int: {}", n)
            }
            0x03 => {
                let f = reader.f64("float constant")?;
                format!("float: {}", f)
            }
            0x04 => {
                let len = reader.u32("string constant length")? as usize;
                let bytes = reader.take(len, "string constant bytes")?;
                let s = String::from_utf8_lossy(bytes);
                format!("string: {:?}", s)
            }
            0x05 => {
                let offset = reader.i64("function constant")?;
                format!("function: @{}", offset)
            }
            _ => {
                return Err(format!(
                    "Unknown constant tag 0x{:02X} at offset {}",
                    tag, tag_offset
                ));
            }
        };
        output.push_str(&format!("  [{:4}] {}\n", i, value_str));
    }
    output.push('\n');

    // Code section
    let code_len = reader.u32("code length")? as usize;
    let code_start = reader.position();
    let code_end = code_start
        .checked_add(code_len)
        .ok_or_else(|| "Code section length overflows input size".to_string())?;
    reader.set_end(code_end)?;

    output.push_str(&format!("=== CODE ({} bytes) ===\n", code_len));

    while reader.position() < code_end {
        let offset = reader.position() - code_start;
        let op_byte = reader.u8("opcode")?;

        let opcode = Opcode::from_byte(op_byte)
            .ok_or_else(|| format!("Unknown opcode 0x{:02X} at code offset {}", op_byte, offset))?;
        let op_name = format!("{:?}", opcode);

        // Handle operands based on opcode
        let operands = match opcode {
            Opcode::LoadConst | Opcode::LoadLocal | Opcode::StoreLocal => {
                let operand = reader.u16("u16 operand")?;
                format!(" {}", operand)
            }
            Opcode::Jump | Opcode::JumpIfTrue | Opcode::JumpIfFalse => {
                let rel = reader.i16("jump offset")?;
                let target = relative_target(code_start, reader.position(), rel, code_len)?;
                format!(" -> {}", target)
            }
            Opcode::Call | Opcode::TailCall => {
                let arg_count = reader.u8("call argument count")?;
                format!(" ({})", arg_count)
            }
            Opcode::Spawn => {
                let code_offset = reader.u16("spawn code offset")?;
                let arg_count = reader.u8("spawn argument count")?;
                format!(" fn:{} ({})", code_offset, arg_count)
            }
            Opcode::GetField | Opcode::SetField => {
                let field_idx = reader.u16("field constant index")?;
                format!(" .{}", field_idx)
            }
            Opcode::MakeClosure => {
                let func_idx = reader.u16("closure code offset")?;
                let capture_count = reader.u8("closure capture count")?;
                format!(" fn:{} captures:{}", func_idx, capture_count)
            }
            Opcode::InterfaceBox => {
                let flags = reader.u8("InterfaceBox flags")?;
                let method_count = reader.u8("InterfaceBox method count")? as usize;
                let witnesses =
                    reader.interface_witnesses(method_count, "InterfaceBox", true, code_start)?;
                format!(
                    " flags:{} methods:{} [{}]",
                    flags,
                    method_count,
                    witnesses.join(", ")
                )
            }
            Opcode::InterfaceCall => {
                let method_name_const = reader.u16("InterfaceCall method name constant")?;
                let arg_count = reader.u8("InterfaceCall argument count")?;
                format!(" name:{} args:{}", method_name_const, arg_count)
            }
            Opcode::LoadCapture => {
                let idx = reader.u8("capture index")?;
                format!(" {}", idx)
            }
            Opcode::Select => {
                let arm_count = reader.u8("select arm count")? as usize;
                let tags: Vec<u8> = (0..arm_count)
                    .map(|_| reader.u8("select arm tag"))
                    .collect::<Result<_, _>>()?;
                if let Some(tag) = tags.iter().find(|tag| **tag > 2) {
                    return Err(format!("Unknown select arm tag {}", tag));
                }
                let mut targets = Vec::with_capacity(arm_count);
                for _ in 0..arm_count {
                    let rel = reader.i16("select body offset")?;
                    targets.push(relative_target(
                        code_start,
                        reader.position(),
                        rel,
                        code_len,
                    )?);
                }
                let arms: Vec<String> = tags
                    .iter()
                    .zip(targets.iter())
                    .map(|(tag, target)| {
                        let kind = match tag {
                            0 => "recv",
                            1 => "send",
                            2 => "default",
                            _ => "?",
                        };
                        format!("{}->{}", kind, target)
                    })
                    .collect();
                format!(" arms:{} [{}]", arm_count, arms.join(", "))
            }
            Opcode::TypeIs | Opcode::Cast => {
                let type_id = reader.u8("type id")?;
                let type_name = match type_id {
                    0 => "null",
                    1 => "bool",
                    2 => "int",
                    3 => "float",
                    4 => "string",
                    5 => "array",
                    6 => "object",
                    7 => "function",
                    8 => "tuple",
                    _ => "unknown",
                };
                format!(" {}", type_name)
            }
            Opcode::InterfaceIs => {
                let source_type = reader.u8("InterfaceIs source type")?;
                let method_count = reader.u8("InterfaceIs method count")? as usize;
                let witnesses =
                    reader.interface_witnesses(method_count, "InterfaceIs", false, code_start)?;
                format!(
                    " source:{} methods:{} [{}]",
                    source_type,
                    method_count,
                    witnesses.join(", ")
                )
            }
            Opcode::Syscall => {
                let syscall_id = reader.u8("syscall id")?;
                format!(" #{}", syscall_id)
            }
            _ => String::new(),
        };

        // Mark entry point
        let marker = if offset as u32 == entry_point {
            " <- entry"
        } else {
            ""
        };
        output.push_str(&format!(
            "  {:5}: {}{}{}\n",
            offset, op_name, operands, marker
        ));
    }

    Ok(output)
}

/// Resolve a signed bytecode-relative target into a code-section offset.
/// Jump and select offsets are relative to the byte after their i16 operand.
fn relative_target(
    code_start: usize,
    operand_end: usize,
    relative: i16,
    code_len: usize,
) -> Result<usize, String> {
    let operand_offset = operand_end
        .checked_sub(code_start)
        .ok_or_else(|| "Instruction offset precedes code section".to_string())?;
    let target = operand_offset as isize + relative as isize;
    if target < 0 || target as usize > code_len {
        return Err(format!(
            "Branch target {} is outside code section [0, {}]",
            target, code_len
        ));
    }
    Ok(target as usize)
}

/// Parse -o/--output argument, defaulting to input file with .lic extension
fn parse_output_arg(args: &[String], input: &str) -> String {
    // Default: replace .li with .lic
    let default = if input.ends_with(".li") {
        format!("{}c", input)
    } else {
        format!("{}.lic", input)
    };
    parse_output_arg_with_default(args, &default)
}

fn parse_output_arg_with_default(args: &[String], default: &str) -> String {
    for i in 3..args.len() {
        if (args[i] == "-o" || args[i] == "--output") && i + 1 < args.len() {
            return args[i + 1].clone();
        }
    }
    default.to_string()
}

/// Default executable name for `lira build`: the source path without its
/// extension, so `examples/hello.li` becomes `examples/hello`.
fn native_output_name(input: &str) -> String {
    match input.strip_suffix(".li") {
        Some(stem) => stem.to_string(),
        None => format!("{}.out", input),
    }
}

fn print_usage() {
    println!(
        r#"Lira Programming Language CLI

USAGE:
    lira <COMMAND> [OPTIONS]

COMMANDS:
    run <file.li>              Compile and execute a Lira program on the bytecode VM
    build <file.li> [OPTS]     Compile to a standalone native executable
    jit <file.li>              Compile to native code in an isolated worker
    compile <file.li> [OPTS]   Compile source to bytecode
    check <file.li>            Type check source without compiling
    ast <file.li>              Dump parsed AST as JSON
    disasm <file.lic>          Disassemble bytecode to human-readable form
    help                       Show this help message
    version                    Show version information

COMPILE OPTIONS:
    -o, --output <file>        Output file
                               (compile: <input>.lic, build: <input> without .li)

EXAMPLES:
    lira run examples/hello.li
    lira build examples/hello.li -o hello && ./hello
    lira jit examples/hello.li
    lira compile main.li -o app.lic
    lira check src/main.li
    lira ast examples/hello.li > hello.json
    lira disasm hello.lic"#
    );
}

#[cfg(test)]
mod tests {
    use super::disassemble;
    use lira_core::opcode::Opcode;

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
    fn interface_call_truncation_identifies_operand() {
        let code = [Opcode::InterfaceCall as u8, 0];
        let error = disassemble(&bytecode_with_code(&code)).expect_err("bytecode is truncated");
        assert!(error.contains("Unexpected end of bytecode"), "{error}");
        assert!(
            error.contains("InterfaceCall method name constant"),
            "{error}"
        );
    }

    #[test]
    fn interface_is_invalid_witness_kind_identifies_method() {
        let code = [Opcode::InterfaceIs as u8, u8::MAX, 1, 0, 0, 7];
        let error = disassemble(&bytecode_with_code(&code)).expect_err("kind is invalid");
        assert!(
            error.contains("Invalid InterfaceIs witness kind 7"),
            "{error}"
        );
        assert!(error.contains("method 0"), "{error}");
    }
}
