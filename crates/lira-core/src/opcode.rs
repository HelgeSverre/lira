//! Lira VM opcodes
//!
//! Defines all bytecode instructions for the Lira virtual machine.
//! See docs/lira/11-instruction-set.md for the full specification.

/// Lira VM opcodes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Opcode {
    // Stack operations
    Nop = 0x00,
    LoadConst = 0x01,
    Pop = 0x02,
    Dup = 0x03,

    // Local variable operations
    LoadLocal = 0x10,
    StoreLocal = 0x11,

    // Untyped arithmetic operations (0x20-0x26)
    Add = 0x20,
    Sub = 0x21,
    Mul = 0x22,
    Div = 0x23,
    Mod = 0x24,
    Neg = 0x25,
    Pow = 0x26,

    // Untyped comparison operations (0x30-0x35)
    Eq = 0x30,
    Ne = 0x31,
    Lt = 0x32,
    Le = 0x33,
    Gt = 0x34,
    Ge = 0x35,

    // Logical operations
    And = 0x40,
    Or = 0x41,
    Not = 0x42,

    // Bitwise operations
    BitAnd = 0x43,
    BitOr = 0x44,
    BitXor = 0x45,
    BitNot = 0x46,
    Shl = 0x47,
    Shr = 0x48,
    UShr = 0x49,

    // Control flow
    Jump = 0x50,
    JumpIfTrue = 0x51,
    JumpIfFalse = 0x52,

    // Function operations
    Call = 0x60,
    Return = 0x61,
    /// Tail call: reuses the current frame instead of pushing a new one.
    /// Follows the same encoding as Call (u8 arg_count) but the callee's
    /// Return will return to the *current* frame's caller.
    TailCall = 0x62,

    // Object operations
    GetField = 0x70,
    SetField = 0x71,
    NewObject = 0x72,

    // Array operations
    NewArray = 0x80,
    ArrayGet = 0x81,
    ArraySet = 0x82,
    ArrayLen = 0x83,
    ArrayPush = 0x84,
    ArrayPop = 0x85,

    // Fiber operations
    Spawn = 0x90,
    Yield = 0x91,
    FiberId = 0x92,

    // Channel operations
    ChanNew = 0xA0,
    ChanSend = 0xA1,
    ChanRecv = 0xA2,
    ChanClose = 0xA3,
    ChanTryRecv = 0xA4,
    Select = 0xA5,

    // Closure operations
    MakeClosure = 0xB0,
    LoadCapture = 0xB1,

    // Type operations
    TypeIs = 0xC0,
    Cast = 0xC1,

    // Typed integer arithmetic (0xD0-0xD5)
    IAdd = 0xD0,
    ISub = 0xD1,
    IMul = 0xD2,
    IDiv = 0xD3,
    IMod = 0xD4,
    INeg = 0xD5,

    // Typed float arithmetic (0xD6-0xDB)
    FAdd = 0xD6,
    FSub = 0xD7,
    FMul = 0xD8,
    FDiv = 0xD9,
    FMod = 0xDA,
    FNeg = 0xDB,

    // Typed integer comparison (0xE0-0xE5)
    IEq = 0xE0,
    INe = 0xE1,
    ILt = 0xE2,
    ILe = 0xE3,
    IGt = 0xE4,
    IGe = 0xE5,

    // Typed float comparison (0xE6-0xEB)
    FEq = 0xE6,
    FNe = 0xE7,
    FLt = 0xE8,
    FLe = 0xE9,
    FGt = 0xEA,
    FGe = 0xEB,

    // System operations
    Print = 0xF0,
    Collect = 0xFD,
    Syscall = 0xFE,
    Halt = 0xFF,
}

impl Opcode {
    pub fn from_byte(byte: u8) -> Option<Self> {
        OPCODE_TABLE[byte as usize]
    }
}

const N: Option<Opcode> = None;

static OPCODE_TABLE: [Option<Opcode>; 256] = [
    // 0x00-0x0F: Stack ops + gaps
    Some(Opcode::Nop),        // 0x00
    Some(Opcode::LoadConst),  // 0x01
    Some(Opcode::Pop),        // 0x02
    Some(Opcode::Dup),        // 0x03
    N, N, N, N, N, N, N, N, N, N, N, N, // 0x04-0x0F
    // 0x10-0x1F: Local ops + gaps
    Some(Opcode::LoadLocal),  // 0x10
    Some(Opcode::StoreLocal), // 0x11
    N, N, N, N, N, N, N, N, N, N, N, N, N, N, // 0x12-0x1F
    // 0x20-0x2F: Arithmetic ops
    Some(Opcode::Add),  // 0x20
    Some(Opcode::Sub),  // 0x21
    Some(Opcode::Mul),  // 0x22
    Some(Opcode::Div),  // 0x23
    Some(Opcode::Mod),  // 0x24
    Some(Opcode::Neg),  // 0x25
    Some(Opcode::Pow),  // 0x26
    N, N, N, N, N, N, N, N, N, // 0x27-0x2F
    // 0x30-0x3F: Comparison ops
    Some(Opcode::Eq), // 0x30
    Some(Opcode::Ne), // 0x31
    Some(Opcode::Lt), // 0x32
    Some(Opcode::Le), // 0x33
    Some(Opcode::Gt), // 0x34
    Some(Opcode::Ge), // 0x35
    N, N, N, N, N, N, N, N, N, N, // 0x36-0x3F
    // 0x40-0x4F: Logical + bitwise ops
    Some(Opcode::And),    // 0x40
    Some(Opcode::Or),     // 0x41
    Some(Opcode::Not),    // 0x42
    Some(Opcode::BitAnd), // 0x43
    Some(Opcode::BitOr),  // 0x44
    Some(Opcode::BitXor), // 0x45
    Some(Opcode::BitNot), // 0x46
    Some(Opcode::Shl),    // 0x47
    Some(Opcode::Shr),    // 0x48
    Some(Opcode::UShr),   // 0x49
    N, N, N, N, N, N, // 0x4A-0x4F
    // 0x50-0x5F: Control flow
    Some(Opcode::Jump),        // 0x50
    Some(Opcode::JumpIfTrue),  // 0x51
    Some(Opcode::JumpIfFalse), // 0x52
    N, N, N, N, N, N, N, N, N, N, N, N, N, // 0x53-0x5F
    // 0x60-0x6F: Functions
    Some(Opcode::Call),   // 0x60
    Some(Opcode::Return), // 0x61
    Some(Opcode::TailCall), // 0x62
    N, N, N, N, N, N, N, N, N, N, N, N, N, // 0x63-0x6F
    // 0x70-0x7F: Objects
    Some(Opcode::GetField),  // 0x70
    Some(Opcode::SetField),  // 0x71
    Some(Opcode::NewObject), // 0x72
    N, N, N, N, N, N, N, N, N, N, N, N, N, // 0x73-0x7F
    // 0x80-0x8F: Arrays
    Some(Opcode::NewArray),  // 0x80
    Some(Opcode::ArrayGet),  // 0x81
    Some(Opcode::ArraySet),  // 0x82
    Some(Opcode::ArrayLen),  // 0x83
    Some(Opcode::ArrayPush), // 0x84
    Some(Opcode::ArrayPop),  // 0x85
    N, N, N, N, N, N, N, N, N, N, // 0x86-0x8F
    // 0x90-0x9F: Fiber ops
    Some(Opcode::Spawn),   // 0x90
    Some(Opcode::Yield),   // 0x91
    Some(Opcode::FiberId), // 0x92
    N, N, N, N, N, N, N, N, N, N, N, N, N, // 0x93-0x9F
    // 0xA0-0xAF: Channel ops
    Some(Opcode::ChanNew),     // 0xA0
    Some(Opcode::ChanSend),    // 0xA1
    Some(Opcode::ChanRecv),    // 0xA2
    Some(Opcode::ChanClose),   // 0xA3
    Some(Opcode::ChanTryRecv), // 0xA4
    Some(Opcode::Select),      // 0xA5
    N, N, N, N, N, N, N, N, N, N, // 0xA6-0xAF
    // 0xB0-0xBF: Closure ops
    Some(Opcode::MakeClosure), // 0xB0
    Some(Opcode::LoadCapture), // 0xB1
    N, N, N, N, N, N, N, N, N, N, N, N, N, N, // 0xB2-0xBF
    // 0xC0-0xCF: Type ops
    Some(Opcode::TypeIs), // 0xC0
    Some(Opcode::Cast),   // 0xC1
    N, N, N, N, N, N, N, N, N, N, N, N, N, N, // 0xC2-0xCF
    // 0xD0-0xDF: Typed integer arithmetic
    Some(Opcode::IAdd), // 0xD0
    Some(Opcode::ISub), // 0xD1
    Some(Opcode::IMul), // 0xD2
    Some(Opcode::IDiv), // 0xD3
    Some(Opcode::IMod), // 0xD4
    Some(Opcode::INeg), // 0xD5
    // Typed float arithmetic
    Some(Opcode::FAdd), // 0xD6
    Some(Opcode::FSub), // 0xD7
    Some(Opcode::FMul), // 0xD8
    Some(Opcode::FDiv), // 0xD9
    Some(Opcode::FMod), // 0xDA
    Some(Opcode::FNeg), // 0xDB
    N, N, N, N, // 0xDC-0xDF
    // 0xE0-0xEF: Typed comparisons
    Some(Opcode::IEq), // 0xE0
    Some(Opcode::INe), // 0xE1
    Some(Opcode::ILt), // 0xE2
    Some(Opcode::ILe), // 0xE3
    Some(Opcode::IGt), // 0xE4
    Some(Opcode::IGe), // 0xE5
    Some(Opcode::FEq), // 0xE6
    Some(Opcode::FNe), // 0xE7
    Some(Opcode::FLt), // 0xE8
    Some(Opcode::FLe), // 0xE9
    Some(Opcode::FGt), // 0xEA
    Some(Opcode::FGe), // 0xEB
    N, N, N, N, // 0xEC-0xEF
    // 0xF0-0xFF: System ops
    Some(Opcode::Print), // 0xF0
    N, N, N, N, N, N, N, N, N, N, N, N, // 0xF1-0xFC
    Some(Opcode::Collect), // 0xFD
    Some(Opcode::Syscall), // 0xFE
    Some(Opcode::Halt),    // 0xFF
];
