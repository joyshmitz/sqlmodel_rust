//! Field and column definitions.

use crate::types::SqlType;

/// Metadata about a model field/column.
#[derive(Debug, Clone)]
pub struct FieldInfo {
    /// Rust field name
    pub name: &'static str,
    /// Database column name (may differ from field name)
    pub column_name: &'static str,
    /// SQL type for this field
    pub sql_type: SqlType,
    /// Whether this field is nullable
    pub nullable: bool,
    /// Whether this is a primary key
    pub primary_key: bool,
    /// Whether this field auto-increments
    pub auto_increment: bool,
    /// Whether this field has a unique constraint
    pub unique: bool,
    /// Default value expression (SQL)
    pub default: Option<&'static str>,
    /// Foreign key reference (table.column)
    pub foreign_key: Option<&'static str>,
    /// Index name if indexed
    pub index: Option<&'static str>,
}

impl FieldInfo {
    /// Create a new field info with minimal required data.
    pub const fn new(name: &'static str, sql_type: SqlType) -> Self {
        Self {
            name,
            column_name: name,
            sql_type,
            nullable: false,
            primary_key: false,
            auto_increment: false,
            unique: false,
            default: None,
            foreign_key: None,
            index: None,
        }
    }

    /// Set the database column name.
    pub const fn column(mut self, name: &'static str) -> Self {
        self.column_name = name;
        self
    }

    /// Mark as nullable.
    pub const fn nullable(mut self) -> Self {
        self.nullable = true;
        self
    }

    /// Mark as primary key.
    pub const fn primary_key(mut self) -> Self {
        self.primary_key = true;
        self
    }

    /// Mark as auto-increment.
    pub const fn auto_increment(mut self) -> Self {
        self.auto_increment = true;
        self
    }

    /// Mark as unique.
    pub const fn unique(mut self) -> Self {
        self.unique = true;
        self
    }

    /// Set default value.
    pub const fn default(mut self, expr: &'static str) -> Self {
        self.default = Some(expr);
        self
    }

    /// Set foreign key reference.
    pub const fn foreign_key(mut self, reference: &'static str) -> Self {
        self.foreign_key = Some(reference);
        self
    }

    /// Set index name.
    pub const fn index(mut self, name: &'static str) -> Self {
        self.index = Some(name);
        self
    }
}

/// A column reference used in queries.
#[derive(Debug, Clone)]
pub struct Column {
    /// Table name (optional, for joins)
    pub table: Option<String>,
    /// Column name
    pub name: String,
    /// Alias (AS name)
    pub alias: Option<String>,
}

impl Column {
    /// Create a new column reference.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            table: None,
            name: name.into(),
            alias: None,
        }
    }

    /// Create a column reference with table prefix.
    pub fn qualified(table: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            table: Some(table.into()),
            name: name.into(),
            alias: None,
        }
    }

    /// Set an alias for this column.
    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.alias = Some(alias.into());
        self
    }

    /// Generate SQL for this column reference.
    pub fn to_sql(&self) -> String {
        let mut sql = if let Some(table) = &self.table {
            format!("{}.{}", table, self.name)
        } else {
            self.name.clone()
        };

        if let Some(alias) = &self.alias {
            sql.push_str(" AS ");
            sql.push_str(alias);
        }

        sql
    }
}

/// A field reference for type-safe column access.
///
/// This is used by generated code to provide compile-time
/// checked column references.
#[derive(Debug, Clone, Copy)]
pub struct Field<T> {
    /// The column name
    pub name: &'static str,
    /// Phantom data for the field type
    _marker: std::marker::PhantomData<T>,
}

impl<T> Field<T> {
    /// Create a new typed field reference.
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            _marker: std::marker::PhantomData,
        }
    }
}
