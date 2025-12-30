//! Lira VM opcodes
//!
//! Defines all bytecode instructions for the Lira virtual machine.
//! See docs/lira/11-instruction-set.md for the full specification.

/// Lira VM opcodes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Opcode {
    // Stack operations
    /// No operation
    Nop = 0x00,
    /// Push constant from pool
    LoadConst = 0x01,
    /// Pop and discard top of stack
    Pop = 0x02,
    /// Duplicate top of stack
    Dup = 0x03,

    // Local variable operations
    /// Load local variable
    LoadLocal = 0x10,
    /// Store to local variable
    StoreLocal = 0x11,

    // Arithmetic operations
    /// Add two values
    Add = 0x20,
    /// Subtract two values
    Sub = 0x21,
    /// Multiply two values
    Mul = 0x22,
    /// Divide two values
    Div = 0x23,
    /// Modulo operation
    Mod = 0x24,
    /// Negate value
    Neg = 0x25,
    /// Power/exponentiation
    Pow = 0x26,

    // Comparison operations
    /// Equal
    Eq = 0x30,
    /// Not equal
    Ne = 0x31,
    /// Less than
    Lt = 0x32,
    /// Less than or equal
    Le = 0x33,
    /// Greater than
    Gt = 0x34,
    /// Greater than or equal
    Ge = 0x35,

    // Logical operations
    /// Logical and
    And = 0x40,
    /// Logical or
    Or = 0x41,
    /// Logical not
    Not = 0x42,

    // Bitwise operations
    /// Bitwise and
    BitAnd = 0x43,
    /// Bitwise or
    BitOr = 0x44,
    /// Bitwise xor
    BitXor = 0x45,
    /// Bitwise not
    BitNot = 0x46,
    /// Shift left
    Shl = 0x47,
    /// Shift right (arithmetic)
    Shr = 0x48,
    /// Shift right (logical/unsigned)
    UShr = 0x49,

    // Control flow
    /// Unconditional jump
    Jump = 0x50,
    /// Jump if true
    JumpIfTrue = 0x51,
    /// Jump if false
    JumpIfFalse = 0x52,

    // Function operations
    /// Call function
    Call = 0x60,
    /// Return from function
    Return = 0x61,

    // Object operations
    /// Get field from object
    GetField = 0x70,
    /// Set field on object
    SetField = 0x71,
    /// Create new object
    NewObject = 0x72,

    // Array operations
    /// Create new array
    NewArray = 0x80,
    /// Get array element
    ArrayGet = 0x81,
    /// Set array element
    ArraySet = 0x82,
    /// Get array length
    ArrayLen = 0x83,
    /// Push element to array end
    ArrayPush = 0x84,
    /// Pop element from array end
    ArrayPop = 0x85,

    // Fiber operations
    /// Spawn a new fiber
    Spawn = 0x90,
    /// Yield current fiber
    Yield = 0x91,
    /// Get current fiber ID
    FiberId = 0x92,

    // Channel operations
    /// Create a new channel
    ChanNew = 0xA0,
    /// Send value on channel
    ChanSend = 0xA1,
    /// Receive value from channel
    ChanRecv = 0xA2,
    /// Close channel
    ChanClose = 0xA3,
    /// Try receive (non-blocking)
    ChanTryRecv = 0xA4,
    /// Select on multiple channels
    Select = 0xA5,

    // Closure operations
    /// Create closure: takes function offset and capture count, pops N capture values from stack
    MakeClosure = 0xB0,
    /// Load captured variable by index
    LoadCapture = 0xB1,

    // Type operations
    /// Check if value is of type (type id as u8 operand): 0=null, 1=bool, 2=int, 3=float, 4=string, 5=array, 6=object
    TypeIs = 0xC0,
    /// Cast value to type (type id as u8 operand)
    Cast = 0xC1,

    // System operations
    /// Print value (debug)
    Print = 0xF0,
    /// System call
    Syscall = 0xFE,
    /// Halt execution
    Halt = 0xFF,
}

impl Opcode {
    /// Try to convert a byte to an opcode
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x00 => Some(Opcode::Nop),
            0x01 => Some(Opcode::LoadConst),
            0x02 => Some(Opcode::Pop),
            0x03 => Some(Opcode::Dup),
            0x10 => Some(Opcode::LoadLocal),
            0x11 => Some(Opcode::StoreLocal),
            0x20 => Some(Opcode::Add),
            0x21 => Some(Opcode::Sub),
            0x22 => Some(Opcode::Mul),
            0x23 => Some(Opcode::Div),
            0x24 => Some(Opcode::Mod),
            0x25 => Some(Opcode::Neg),
            0x26 => Some(Opcode::Pow),
            0x30 => Some(Opcode::Eq),
            0x31 => Some(Opcode::Ne),
            0x32 => Some(Opcode::Lt),
            0x33 => Some(Opcode::Le),
            0x34 => Some(Opcode::Gt),
            0x35 => Some(Opcode::Ge),
            0x40 => Some(Opcode::And),
            0x41 => Some(Opcode::Or),
            0x42 => Some(Opcode::Not),
            0x43 => Some(Opcode::BitAnd),
            0x44 => Some(Opcode::BitOr),
            0x45 => Some(Opcode::BitXor),
            0x46 => Some(Opcode::BitNot),
            0x47 => Some(Opcode::Shl),
            0x48 => Some(Opcode::Shr),
            0x49 => Some(Opcode::UShr),
            0x50 => Some(Opcode::Jump),
            0x51 => Some(Opcode::JumpIfTrue),
            0x52 => Some(Opcode::JumpIfFalse),
            0x60 => Some(Opcode::Call),
            0x61 => Some(Opcode::Return),
            0x70 => Some(Opcode::GetField),
            0x71 => Some(Opcode::SetField),
            0x72 => Some(Opcode::NewObject),
            0x80 => Some(Opcode::NewArray),
            0x81 => Some(Opcode::ArrayGet),
            0x82 => Some(Opcode::ArraySet),
            0x83 => Some(Opcode::ArrayLen),
            0x84 => Some(Opcode::ArrayPush),
            0x85 => Some(Opcode::ArrayPop),
            0x90 => Some(Opcode::Spawn),
            0x91 => Some(Opcode::Yield),
            0x92 => Some(Opcode::FiberId),
            0xA0 => Some(Opcode::ChanNew),
            0xA1 => Some(Opcode::ChanSend),
            0xA2 => Some(Opcode::ChanRecv),
            0xA3 => Some(Opcode::ChanClose),
            0xA4 => Some(Opcode::ChanTryRecv),
            0xA5 => Some(Opcode::Select),
            0xB0 => Some(Opcode::MakeClosure),
            0xB1 => Some(Opcode::LoadCapture),
            0xC0 => Some(Opcode::TypeIs),
            0xC1 => Some(Opcode::Cast),
            0xF0 => Some(Opcode::Print),
            0xFE => Some(Opcode::Syscall),
            0xFF => Some(Opcode::Halt),
            _ => None,
        }
    }
}
