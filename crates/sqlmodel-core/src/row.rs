//! Database row representation.

use crate::Result;
use crate::error::{Error, TypeError};
use crate::value::Value;
use std::collections::HashMap;

/// A single row returned from a database query.
///
/// Rows provide both index-based and name-based access to column values.
#[derive(Debug, Clone)]
pub struct Row {
    /// Column values in order
    values: Vec<Value>,
    /// Column name to index mapping
    columns: HashMap<String, usize>,
}

impl Row {
    /// Create a new row with the given columns and values.
    pub fn new(column_names: Vec<String>, values: Vec<Value>) -> Self {
        let columns = column_names
            .into_iter()
            .enumerate()
            .map(|(i, name)| (name, i))
            .collect();

        Self { values, columns }
    }

    /// Get the number of columns in this row.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Check if this row is empty.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Get a value by column index.
    pub fn get(&self, index: usize) -> Option<&Value> {
        self.values.get(index)
    }

    /// Get a value by column name.
    pub fn get_by_name(&self, name: &str) -> Option<&Value> {
        self.columns.get(name).and_then(|&i| self.values.get(i))
    }

    /// Get a typed value by column index.
    pub fn get_as<T: FromValue>(&self, index: usize) -> Result<T> {
        let value = self.get(index).ok_or_else(|| {
            Error::Type(TypeError {
                expected: std::any::type_name::<T>().to_string(),
                found: "index out of bounds".to_string(),
                column: None,
            })
        })?;
        T::from_value(value)
    }

    /// Get a typed value by column name.
    pub fn get_named<T: FromValue>(&self, name: &str) -> Result<T> {
        let value = self.get_by_name(name).ok_or_else(|| {
            Error::Type(TypeError {
                expected: std::any::type_name::<T>().to_string(),
                found: "column not found".to_string(),
                column: Some(name.to_string()),
            })
        })?;
        T::from_value(value).map_err(|e| match e {
            Error::Type(mut te) => {
                te.column = Some(name.to_string());
                Error::Type(te)
            }
            e => e,
        })
    }

    /// Get all column names.
    pub fn column_names(&self) -> impl Iterator<Item = &str> {
        let mut names: Vec<_> = self.columns.iter().collect();
        names.sort_by_key(|(_, i)| *i);
        names.into_iter().map(|(name, _)| name.as_str())
    }

    /// Iterate over (column_name, value) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        let mut pairs: Vec<_> = self.columns.iter().collect();
        pairs.sort_by_key(|(_, i)| *i);
        pairs
            .into_iter()
            .map(|(name, i)| (name.as_str(), &self.values[*i]))
    }
}

/// Trait for converting from a `Value` to a typed value.
pub trait FromValue: Sized {
    /// Convert from a Value, returning an error if the conversion fails.
    fn from_value(value: &Value) -> Result<Self>;
}

impl FromValue for bool {
    fn from_value(value: &Value) -> Result<Self> {
        value.as_bool().ok_or_else(|| {
            Error::Type(TypeError {
                expected: "bool".to_string(),
                found: value.type_name().to_string(),
                column: None,
            })
        })
    }
}

impl FromValue for i32 {
    fn from_value(value: &Value) -> Result<Self> {
        match value {
            Value::TinyInt(v) => Ok(i32::from(*v)),
            Value::SmallInt(v) => Ok(i32::from(*v)),
            Value::Int(v) => Ok(*v),
            _ => Err(Error::Type(TypeError {
                expected: "i32".to_string(),
                found: value.type_name().to_string(),
                column: None,
            })),
        }
    }
}

impl FromValue for i64 {
    fn from_value(value: &Value) -> Result<Self> {
        value.as_i64().ok_or_else(|| {
            Error::Type(TypeError {
                expected: "i64".to_string(),
                found: value.type_name().to_string(),
                column: None,
            })
        })
    }
}

impl FromValue for f64 {
    fn from_value(value: &Value) -> Result<Self> {
        value.as_f64().ok_or_else(|| {
            Error::Type(TypeError {
                expected: "f64".to_string(),
                found: value.type_name().to_string(),
                column: None,
            })
        })
    }
}

impl FromValue for String {
    fn from_value(value: &Value) -> Result<Self> {
        match value {
            Value::Text(s) => Ok(s.clone()),
            Value::Decimal(s) => Ok(s.clone()),
            _ => Err(Error::Type(TypeError {
                expected: "String".to_string(),
                found: value.type_name().to_string(),
                column: None,
            })),
        }
    }
}

impl FromValue for Vec<u8> {
    fn from_value(value: &Value) -> Result<Self> {
        match value {
            Value::Bytes(b) => Ok(b.clone()),
            _ => Err(Error::Type(TypeError {
                expected: "Vec<u8>".to_string(),
                found: value.type_name().to_string(),
                column: None,
            })),
        }
    }
}

impl<T: FromValue> FromValue for Option<T> {
    fn from_value(value: &Value) -> Result<Self> {
        if value.is_null() {
            Ok(None)
        } else {
            T::from_value(value).map(Some)
        }
    }
}

impl FromValue for Value {
    fn from_value(value: &Value) -> Result<Self> {
        Ok(value.clone())
    }
}
