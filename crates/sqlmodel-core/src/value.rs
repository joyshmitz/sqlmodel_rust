//! Dynamic SQL values.

use serde::{Deserialize, Serialize};

/// A dynamically-typed SQL value.
///
/// This enum represents all possible SQL values and is used
/// for parameter binding and result fetching.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    /// NULL value
    Null,

    /// Boolean value
    Bool(bool),

    /// 8-bit signed integer
    TinyInt(i8),

    /// 16-bit signed integer
    SmallInt(i16),

    /// 32-bit signed integer
    Int(i32),

    /// 64-bit signed integer
    BigInt(i64),

    /// 32-bit floating point
    Float(f32),

    /// 64-bit floating point
    Double(f64),

    /// Arbitrary precision decimal (stored as string)
    Decimal(String),

    /// Text string
    Text(String),

    /// Binary data
    Bytes(Vec<u8>),

    /// Date (days since epoch)
    Date(i32),

    /// Time (microseconds since midnight)
    Time(i64),

    /// Timestamp (microseconds since epoch)
    Timestamp(i64),

    /// Timestamp with timezone (microseconds since epoch, UTC)
    TimestampTz(i64),

    /// UUID (as 16 bytes)
    Uuid([u8; 16]),

    /// JSON value
    Json(serde_json::Value),

    /// Array of values
    Array(Vec<Value>),
}

impl Value {
    /// Check if this value is NULL.
    pub const fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Get the type name of this value.
    pub const fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "NULL",
            Value::Bool(_) => "BOOLEAN",
            Value::TinyInt(_) => "TINYINT",
            Value::SmallInt(_) => "SMALLINT",
            Value::Int(_) => "INTEGER",
            Value::BigInt(_) => "BIGINT",
            Value::Float(_) => "REAL",
            Value::Double(_) => "DOUBLE",
            Value::Decimal(_) => "DECIMAL",
            Value::Text(_) => "TEXT",
            Value::Bytes(_) => "BLOB",
            Value::Date(_) => "DATE",
            Value::Time(_) => "TIME",
            Value::Timestamp(_) => "TIMESTAMP",
            Value::TimestampTz(_) => "TIMESTAMPTZ",
            Value::Uuid(_) => "UUID",
            Value::Json(_) => "JSON",
            Value::Array(_) => "ARRAY",
        }
    }

    /// Try to convert this value to a bool.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(v) => Some(*v),
            Value::TinyInt(v) => Some(*v != 0),
            Value::SmallInt(v) => Some(*v != 0),
            Value::Int(v) => Some(*v != 0),
            Value::BigInt(v) => Some(*v != 0),
            _ => None,
        }
    }

    /// Try to convert this value to an i64.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::TinyInt(v) => Some(i64::from(*v)),
            Value::SmallInt(v) => Some(i64::from(*v)),
            Value::Int(v) => Some(i64::from(*v)),
            Value::BigInt(v) => Some(*v),
            Value::Bool(v) => Some(if *v { 1 } else { 0 }),
            _ => None,
        }
    }

    /// Try to convert this value to an f64.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Float(v) => Some(f64::from(*v)),
            Value::Double(v) => Some(*v),
            Value::TinyInt(v) => Some(f64::from(*v)),
            Value::SmallInt(v) => Some(f64::from(*v)),
            Value::Int(v) => Some(f64::from(*v)),
            Value::BigInt(v) => Some(*v as f64),
            _ => None,
        }
    }

    /// Try to get this value as a string reference.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            Value::Decimal(s) => Some(s),
            _ => None,
        }
    }

    /// Try to get this value as a byte slice.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Bytes(b) => Some(b),
            Value::Text(s) => Some(s.as_bytes()),
            _ => None,
        }
    }
}

// Conversion implementations
impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

impl From<i8> for Value {
    fn from(v: i8) -> Self {
        Value::TinyInt(v)
    }
}

impl From<i16> for Value {
    fn from(v: i16) -> Self {
        Value::SmallInt(v)
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Int(v)
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::BigInt(v)
    }
}

impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::Float(v)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Double(v)
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Text(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Text(v.to_string())
    }
}

impl From<Vec<u8>> for Value {
    fn from(v: Vec<u8>) -> Self {
        Value::Bytes(v)
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Self {
        match v {
            Some(v) => v.into(),
            None => Value::Null,
        }
    }
}
