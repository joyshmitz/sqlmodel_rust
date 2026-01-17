//! CREATE TABLE statement builder.

use sqlmodel_core::{FieldInfo, Model};
use std::marker::PhantomData;

/// Builder for CREATE TABLE statements.
#[derive(Debug)]
pub struct CreateTable<M: Model> {
    if_not_exists: bool,
    _marker: PhantomData<M>,
}

impl<M: Model> CreateTable<M> {
    /// Create a new CREATE TABLE builder.
    pub fn new() -> Self {
        Self {
            if_not_exists: false,
            _marker: PhantomData,
        }
    }

    /// Add IF NOT EXISTS clause.
    pub fn if_not_exists(mut self) -> Self {
        self.if_not_exists = true;
        self
    }

    /// Build the CREATE TABLE SQL.
    pub fn build(&self) -> String {
        let mut sql = String::from("CREATE TABLE ");

        if self.if_not_exists {
            sql.push_str("IF NOT EXISTS ");
        }

        sql.push_str(M::TABLE_NAME);
        sql.push_str(" (\n");

        let fields = M::fields();
        let mut column_defs = Vec::new();
        let mut constraints = Vec::new();

        for field in fields {
            column_defs.push(self.column_definition(field));

            // Collect constraints
            if field.unique && !field.primary_key {
                constraints.push(format!(
                    "CONSTRAINT uk_{} UNIQUE ({})",
                    field.column_name, field.column_name
                ));
            }

            if let Some(fk) = field.foreign_key {
                let parts: Vec<&str> = fk.split('.').collect();
                if parts.len() == 2 {
                    constraints.push(format!(
                        "CONSTRAINT fk_{}_{} FOREIGN KEY ({}) REFERENCES {}({})",
                        M::TABLE_NAME,
                        field.column_name,
                        field.column_name,
                        parts[0],
                        parts[1]
                    ));
                }
            }
        }

        // Add primary key constraint
        let pk_cols = M::PRIMARY_KEY;
        if !pk_cols.is_empty() {
            constraints.insert(0, format!("PRIMARY KEY ({})", pk_cols.join(", ")));
        }

        // Combine column definitions and constraints
        let all_parts: Vec<_> = column_defs.into_iter().chain(constraints).collect();

        sql.push_str(&all_parts.join(",\n  "));
        sql.push_str("\n)");

        sql
    }

    fn column_definition(&self, field: &FieldInfo) -> String {
        let mut def = format!("  {} {}", field.column_name, field.sql_type.sql_name());

        if !field.nullable && !field.auto_increment {
            def.push_str(" NOT NULL");
        }

        if field.auto_increment {
            // Use AUTOINCREMENT for SQLite, SERIAL/GENERATED for PostgreSQL
            // For now, use a simple approach
            def.push_str(" AUTOINCREMENT");
        }

        if let Some(default) = field.default {
            def.push_str(" DEFAULT ");
            def.push_str(default);
        }

        def
    }
}

impl<M: Model> Default for CreateTable<M> {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for multiple schema operations.
#[derive(Debug, Default)]
pub struct SchemaBuilder {
    statements: Vec<String>,
}

impl SchemaBuilder {
    /// Create a new schema builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a CREATE TABLE statement.
    pub fn create_table<M: Model>(mut self) -> Self {
        self.statements
            .push(CreateTable::<M>::new().if_not_exists().build());
        self
    }

    /// Add a raw SQL statement.
    pub fn raw(mut self, sql: impl Into<String>) -> Self {
        self.statements.push(sql.into());
        self
    }

    /// Add an index creation statement.
    pub fn create_index(mut self, name: &str, table: &str, columns: &[&str], unique: bool) -> Self {
        let unique_str = if unique { "UNIQUE " } else { "" };
        self.statements.push(format!(
            "CREATE {}INDEX IF NOT EXISTS {} ON {} ({})",
            unique_str,
            name,
            table,
            columns.join(", ")
        ));
        self
    }

    /// Get all SQL statements.
    pub fn build(self) -> Vec<String> {
        self.statements
    }
}
