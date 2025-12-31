//! Lira Value Types
//!
//! Defines all value types used in the Lira virtual machine.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

/// Interned string type - shared reference-counted string
pub type IString = Rc<String>;

/// Fiber ID type
pub type FiberId = u64;

/// Channel ID type
pub type ChannelId = u64;

/// Value types in the VM
#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(IString), // Interned string for memory efficiency
    Array(Rc<RefCell<Vec<Value>>>),
    Object(Rc<RefCell<HashMap<String, Value>>>),
    Function(usize),          // Code offset
    Closure(Rc<ClosureData>), // Closure with captured values
    Fiber(FiberId),           // Fiber handle
    Channel(ChannelId),       // Channel handle
}

/// Closure data containing function code and captured values
#[derive(Debug, Clone)]
pub struct ClosureData {
    /// Offset of the function code
    pub code_offset: usize,
    /// Captured variable values (indexed by capture slot)
    pub captures: Vec<Value>,
}

impl Value {
    /// Check if value is truthy
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Float(f) => *f != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::Array(arr) => !arr.borrow().is_empty(),
            Value::Object(_) => true,
            Value::Function(_) => true,
            Value::Closure(_) => true,
            Value::Fiber(_) => true,
            Value::Channel(_) => true,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Int(n) => write!(f, "{}", n),
            Value::Float(fl) => write!(f, "{}", fl),
            Value::String(s) => write!(f, "{}", &**s),
            Value::Array(arr) => {
                let elements: Vec<String> = arr.borrow().iter().map(|v| v.to_string()).collect();
                write!(f, "[{}]", elements.join(", "))
            }
            Value::Object(obj) => {
                let fields: Vec<String> = obj
                    .borrow()
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v))
                    .collect();
                write!(f, "{{{}}}", fields.join(", "))
            }
            Value::Function(offset) => write!(f, "<function@{}>", offset),
            Value::Closure(c) => write!(f, "<closure@{}>", c.code_offset),
            Value::Fiber(id) => write!(f, "<fiber#{}>", id),
            Value::Channel(id) => write!(f, "<channel#{}>", id),
        }
    }
}
