//! Lira Value Types
//!
//! Defines all value types used in the Lira virtual machine.

use std::cell::RefCell;
use std::collections::HashMap;
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
    String(IString),  // Interned string for memory efficiency
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

    /// Format value for printing
    pub fn to_string(&self) -> String {
        match self {
            Value::Null => "null".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Int(n) => n.to_string(),
            Value::Float(f) => f.to_string(),
            Value::String(s) => (**s).clone(),
            Value::Array(arr) => {
                let elements: Vec<String> = arr.borrow().iter().map(|v| v.to_string()).collect();
                format!("[{}]", elements.join(", "))
            }
            Value::Object(obj) => {
                let fields: Vec<String> = obj
                    .borrow()
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v.to_string()))
                    .collect();
                format!("{{{}}}", fields.join(", "))
            }
            Value::Function(offset) => format!("<function@{}>", offset),
            Value::Closure(c) => format!("<closure@{}>", c.code_offset),
            Value::Fiber(id) => format!("<fiber#{}>", id),
            Value::Channel(id) => format!("<channel#{}>", id),
        }
    }
}
