use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone)]
pub enum RtValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Array(Vec<RtValue>),
    Struct(String, HashMap<String, RtValue>),
    None_,
}

impl RtValue {
    pub fn type_name(&self) -> &str {
        match self {
            RtValue::Int(_) => "int",
            RtValue::Float(_) => "float",
            RtValue::Bool(_) => "bool",
            RtValue::Str(_) => "str",
            RtValue::Array(_) => "array",
            RtValue::Struct(name, _) => name.as_str(),
            RtValue::None_ => "none",
        }
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            RtValue::Bool(b) => *b,
            RtValue::Int(n) => *n != 0,
            RtValue::Float(f) => *f != 0.0,
            RtValue::Str(s) => !s.is_empty(),
            RtValue::Array(a) => !a.is_empty(),
            RtValue::Struct(_, _) => true,
            RtValue::None_ => false,
        }
    }

    pub fn to_string_val(&self) -> String {
        match self {
            RtValue::Int(n) => n.to_string(),
            RtValue::Float(f) => f.to_string(),
            RtValue::Bool(b) => b.to_string(),
            RtValue::Str(s) => s.clone(),
            RtValue::Array(elems) => {
                let items: Vec<String> = elems.iter().map(|v| v.to_string_val()).collect();
                format!("[{}]", items.join(", "))
            }
            RtValue::Struct(name, fields) => {
                let items: Vec<String> = fields
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v.to_string_val()))
                    .collect();
                format!("{} {{ {} }}", name, items.join(", "))
            }
            RtValue::None_ => "none".to_string(),
        }
    }

    pub fn to_number(&self) -> f64 {
        match self {
            RtValue::Int(n) => *n as f64,
            RtValue::Float(f) => *f,
            RtValue::Struct(_, _) => 1.0,
            _ => 0.0,
        }
    }
}

impl fmt::Display for RtValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RtValue::Str(s) => write!(f, "{}", s),
            RtValue::Struct(_, _) => write!(f, "{}", self.to_string_val()),
            _ => write!(f, "{}", self.to_string_val()),
        }
    }
}
