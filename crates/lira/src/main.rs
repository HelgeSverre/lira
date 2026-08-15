//! Lira CLI - Unified interface for Lira programming language
//!
//! Commands:
//!   lira run <file.li>                 Compile and execute Lira source
//!   lira build <file.li> [-o <out>]    Compile to a native executable
//!   lira jit <file.li>                 Compile to native code and run in-process
//!   lira compile <file.li> [-o <out>]  Compile to bytecode
//!   lira check <file.li>               Type check without compiling
//!   lira ast <file.li>                 Dump parsed AST as JSON
//!   lira disasm <file.lic>             Disassemble bytecode
//!   lira --help                        Show help
//!   lira --version                     Show version

use lira_core::opcode::Opcode;
use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

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

/// Compile a Lira source file to native code and run it in this process
fn jit_command(file: &str) -> Result<i32, String> {
    let source = fs::read_to_string(file).map_err(|e| format!("Failed to read {}: {}", file, e))?;
    lira_codegen::jit_run(file, &source)
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
        return Err("Bytecode too short".to_string());
    }

    let mut output = String::new();
    let mut pos = 0;

    // Helper to read bytes
    let read_u8 = |pos: &mut usize| -> Result<u8, String> {
        if *pos >= bytecode.len() {
            return Err("Unexpected end of bytecode".to_string());
        }
        let v = bytecode[*pos];
        *pos += 1;
        Ok(v)
    };

    let read_u16 = |pos: &mut usize| -> Result<u16, String> {
        let lo = bytecode.get(*pos).ok_or("Unexpected end")?;
        let hi = bytecode.get(*pos + 1).ok_or("Unexpected end")?;
        *pos += 2;
        Ok((*lo as u16) | ((*hi as u16) << 8))
    };

    let read_u32 = |pos: &mut usize| -> Result<u32, String> {
        let b0 = bytecode.get(*pos).ok_or("Unexpected end")?;
        let b1 = bytecode.get(*pos + 1).ok_or("Unexpected end")?;
        let b2 = bytecode.get(*pos + 2).ok_or("Unexpected end")?;
        let b3 = bytecode.get(*pos + 3).ok_or("Unexpected end")?;
        *pos += 4;
        Ok((*b0 as u32) | ((*b1 as u32) << 8) | ((*b2 as u32) << 16) | ((*b3 as u32) << 24))
    };

    let read_i64 = |pos: &mut usize| -> Result<i64, String> {
        let lo = read_u32(pos)? as u64;
        let hi = read_u32(pos)? as u64;
        Ok((lo | (hi << 32)) as i64)
    };

    let read_f64 = |pos: &mut usize| -> Result<f64, String> {
        let lo = read_u32(pos)? as u64;
        let hi = read_u32(pos)? as u64;
        Ok(f64::from_bits(lo | (hi << 32)))
    };

    // Parse header
    let magic = read_u32(&mut pos)?;
    let version = read_u32(&mut pos)?;
    let flags = read_u32(&mut pos)?;
    let entry_point = read_u32(&mut pos)?;
    let constant_count = read_u32(&mut pos)?;
    let function_count = read_u32(&mut pos)?;

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
        let tag = read_u8(&mut pos)?;
        let value_str = match tag {
            0x00 => "null".to_string(),
            0x01 => {
                let b = read_u8(&mut pos)?;
                format!("bool: {}", b != 0)
            }
            0x02 => {
                let n = read_i64(&mut pos)?;
                format!("int: {}", n)
            }
            0x03 => {
                let f = read_f64(&mut pos)?;
                format!("float: {}", f)
            }
            0x04 => {
                let len = read_u32(&mut pos)? as usize;
                let bytes = &bytecode[pos..pos + len];
                pos += len;
                let s = String::from_utf8_lossy(bytes);
                format!("string: {:?}", s)
            }
            0x05 => {
                let offset = read_i64(&mut pos)?;
                format!("function: @{}", offset)
            }
            _ => format!("unknown(0x{:02X})", tag),
        };
        output.push_str(&format!("  [{:4}] {}\n", i, value_str));
    }
    output.push('\n');

    // Code section
    let code_len = read_u32(&mut pos)? as usize;
    let code_start = pos;
    let code_end = pos + code_len;

    output.push_str(&format!("=== CODE ({} bytes) ===\n", code_len));

    while pos < code_end {
        let offset = pos - code_start;
        let op_byte = read_u8(&mut pos)?;

        let op_name = if let Some(op) = Opcode::from_byte(op_byte) {
            format!("{:?}", op)
        } else {
            format!("UNKNOWN(0x{:02X})", op_byte)
        };

        // Handle operands based on opcode
        let operands = match Opcode::from_byte(op_byte) {
            Some(Opcode::LoadConst) | Some(Opcode::LoadLocal) | Some(Opcode::StoreLocal) => {
                if pos < code_end {
                    let operand = read_u8(&mut pos)?;
                    format!(" {}", operand)
                } else {
                    String::new()
                }
            }
            Some(Opcode::Jump) | Some(Opcode::JumpIfTrue) | Some(Opcode::JumpIfFalse) => {
                if pos + 1 < code_end {
                    let target = read_u16(&mut pos)?;
                    format!(" -> {}", target)
                } else {
                    String::new()
                }
            }
            Some(Opcode::Call) => {
                if pos < code_end {
                    let arg_count = read_u8(&mut pos)?;
                    format!(" ({})", arg_count)
                } else {
                    String::new()
                }
            }
            Some(Opcode::NewArray) => {
                if pos < code_end {
                    let size = read_u8(&mut pos)?;
                    format!(" [{}]", size)
                } else {
                    String::new()
                }
            }
            Some(Opcode::NewObject) => {
                if pos < code_end {
                    let field_count = read_u8(&mut pos)?;
                    format!(" {{{}}}", field_count)
                } else {
                    String::new()
                }
            }
            Some(Opcode::GetField) | Some(Opcode::SetField) => {
                if pos < code_end {
                    let field_idx = read_u8(&mut pos)?;
                    format!(" .{}", field_idx)
                } else {
                    String::new()
                }
            }
            Some(Opcode::MakeClosure) => {
                if pos + 1 < code_end {
                    let func_idx = read_u8(&mut pos)?;
                    let capture_count = read_u8(&mut pos)?;
                    format!(" fn:{} captures:{}", func_idx, capture_count)
                } else {
                    String::new()
                }
            }
            Some(Opcode::LoadCapture) => {
                if pos < code_end {
                    let idx = read_u8(&mut pos)?;
                    format!(" {}", idx)
                } else {
                    String::new()
                }
            }
            Some(Opcode::ChanNew) => {
                if pos < code_end {
                    let capacity = read_u8(&mut pos)?;
                    format!(" cap:{}", capacity)
                } else {
                    String::new()
                }
            }
            Some(Opcode::Select) => {
                if pos < code_end {
                    let arm_count = read_u8(&mut pos)? as usize;
                    // Decode the tag table.
                    let mut tags = Vec::with_capacity(arm_count);
                    for _ in 0..arm_count {
                        tags.push(read_u8(&mut pos)?);
                    }
                    // Decode the body-offset table (relative i16 per arm).
                    let mut targets = Vec::with_capacity(arm_count);
                    for _ in 0..arm_count {
                        let rel = read_u16(&mut pos)? as i16;
                        let target = (pos as isize + rel as isize) as usize;
                        targets.push(target);
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
                } else {
                    String::new()
                }
            }
            Some(Opcode::TypeIs) | Some(Opcode::Cast) => {
                if pos < code_end {
                    let type_id = read_u8(&mut pos)?;
                    let type_name = match type_id {
                        0 => "null",
                        1 => "bool",
                        2 => "int",
                        3 => "float",
                        4 => "string",
                        5 => "array",
                        6 => "object",
                        _ => "unknown",
                    };
                    format!(" {}", type_name)
                } else {
                    String::new()
                }
            }
            Some(Opcode::Syscall) => {
                if pos < code_end {
                    let syscall_id = read_u8(&mut pos)?;
                    format!(" #{}", syscall_id)
                } else {
                    String::new()
                }
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
    jit <file.li>              Compile to native code and run it in-process
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
