//! Query builders for INSERT, UPDATE, DELETE operations.

use crate::clause::Where;
use crate::expr::{Dialect, Expr};
use asupersync::{Cx, Outcome};
use sqlmodel_core::{Connection, Model, Value};
use std::marker::PhantomData;

/// INSERT query builder.
#[derive(Debug)]
pub struct InsertBuilder<'a, M: Model> {
    model: &'a M,
}

impl<'a, M: Model> InsertBuilder<'a, M> {
    /// Create a new INSERT builder for the given model instance.
    pub fn new(model: &'a M) -> Self {
        Self { model }
    }

    /// Build the INSERT SQL and parameters with default dialect (Postgres).
    pub fn build(&self) -> (String, Vec<Value>) {
        self.build_with_dialect(Dialect::default())
    }

    /// Build the INSERT SQL and parameters with specific dialect.
    pub fn build_with_dialect(&self, dialect: Dialect) -> (String, Vec<Value>) {
        let row = self.model.to_row();
        let fields = M::fields();

        // Filter out auto-increment fields when inserting new records
        let insert_fields: Vec<_> = row
            .iter()
            .filter(|(name, value)| {
                // Skip NULL values for auto-increment fields
                let field = fields.iter().find(|f| f.name == *name);
                if let Some(f) = field {
                    if f.auto_increment && matches!(value, Value::Null) {
                        return false;
                    }
                }
                true
            })
            .collect();

        let columns: Vec<_> = insert_fields.iter().map(|(name, _)| *name).collect();
        let values: Vec<_> = insert_fields
            .iter()
            .map(|(_, value)| value.clone())
            .collect();
        
        let placeholders: Vec<_> = (1..=values.len())
            .map(|i| dialect.placeholder(i))
            .collect();

        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            M::TABLE_NAME,
            columns.join(", "),
            placeholders.join(", ")
        );

        (sql, values)
    }

    /// Execute the INSERT and return the inserted ID.
    pub async fn execute<C: Connection>(
        self,
        cx: &Cx,
        conn: &C,
    ) -> Outcome<i64, sqlmodel_core::Error> {
        // TODO: Get dialect from connection? For now default to Postgres.
        let (sql, params) = self.build();
        conn.insert(cx, &sql, &params).await
    }
}

/// UPDATE query builder.
#[derive(Debug)]
pub struct UpdateBuilder<'a, M: Model> {
    model: &'a M,
    where_clause: Option<Where>,
    set_fields: Option<Vec<&'static str>>,
}

impl<'a, M: Model> UpdateBuilder<'a, M> {
    /// Create a new UPDATE builder for the given model instance.
    pub fn new(model: &'a M) -> Self {
        Self {
            model,
            where_clause: None,
            set_fields: None,
        }
    }

    /// Only update specific fields.
    pub fn set_only(mut self, fields: &[&'static str]) -> Self {
        self.set_fields = Some(fields.to_vec());
        self
    }

    /// Add a WHERE condition (defaults to primary key match).
    pub fn filter(mut self, expr: Expr) -> Self {
        self.where_clause = Some(match self.where_clause {
            Some(existing) => existing.and(expr),
            None => Where::new(expr),
        });
        self
    }

    /// Build the UPDATE SQL and parameters with default dialect (Postgres).
    pub fn build(&self) -> (String, Vec<Value>) {
        self.build_with_dialect(Dialect::default())
    }

    /// Build the UPDATE SQL and parameters with specific dialect.
    pub fn build_with_dialect(&self, dialect: Dialect) -> (String, Vec<Value>) {
        let row = self.model.to_row();
        let pk = M::PRIMARY_KEY;

        // Determine which fields to update
        let update_fields: Vec<_> = row
            .iter()
            .filter(|(name, _)| {
                // Skip primary key fields
                if pk.contains(name) {
                    return false;
                }
                // If set_only specified, only include those fields
                if let Some(fields) = &self.set_fields {
                    return fields.contains(name);
                }
                true
            })
            .collect();

        let mut params = Vec::new();
        let mut set_clauses = Vec::new();

        for (i, (name, value)) in update_fields.iter().enumerate() {
            set_clauses.push(format!("{} = {}", name, dialect.placeholder(i + 1)));
            params.push((*value).clone());
        }

        let mut sql = format!("UPDATE {} SET {}", M::TABLE_NAME, set_clauses.join(", "));

        // Add WHERE clause
        if let Some(where_clause) = &self.where_clause {
            let (where_sql, where_params) = where_clause.build_with_dialect(dialect, params.len());
            sql.push_str(" WHERE ");
            sql.push_str(&where_sql);
            params.extend(where_params);
        } else {
            // Default to primary key match
            let pk_values = self.model.primary_key_value();
            let pk_conditions: Vec<_> = pk
                .iter()
                .zip(pk_values.iter())
                .enumerate()
                .map(|(i, (col, _))| format!("{} = {}", col, dialect.placeholder(params.len() + i + 1)))
                .collect();

            if !pk_conditions.is_empty() {
                sql.push_str(" WHERE ");
                sql.push_str(&pk_conditions.join(" AND "));
                params.extend(pk_values);
            }
        }

        (sql, params)
    }

    /// Execute the UPDATE and return rows affected.
    pub async fn execute<C: Connection>(
        self,
        cx: &Cx,
        conn: &C,
    ) -> Outcome<u64, sqlmodel_core::Error> {
        let (sql, params) = self.build();
        conn.execute(cx, &sql, &params).await
    }
}

/// DELETE query builder.
#[derive(Debug)]
pub struct DeleteBuilder<M: Model> {
    where_clause: Option<Where>,
    _marker: PhantomData<M>,
}

impl<M: Model> DeleteBuilder<M> {
    /// Create a new DELETE builder for the model type.
    pub fn new() -> Self {
        Self {
            where_clause: None,
            _marker: PhantomData,
        }
    }

    /// Add a WHERE condition.
    pub fn filter(mut self, expr: Expr) -> Self {
        self.where_clause = Some(match self.where_clause {
            Some(existing) => existing.and(expr),
            None => Where::new(expr),
        });
        self
    }

    /// Build the DELETE SQL and parameters with default dialect (Postgres).
    pub fn build(&self) -> (String, Vec<Value>) {
        self.build_with_dialect(Dialect::default())
    }

    /// Build the DELETE SQL and parameters with specific dialect.
    pub fn build_with_dialect(&self, dialect: Dialect) -> (String, Vec<Value>) {
        let mut sql = format!("DELETE FROM {}", M::TABLE_NAME);
        let mut params = Vec::new();

        if let Some(where_clause) = &self.where_clause {
            let (where_sql, where_params) = where_clause.build_with_dialect(dialect, 0);
            sql.push_str(" WHERE ");
            sql.push_str(&where_sql);
            params = where_params;
        }

        (sql, params)
    }

    /// Execute the DELETE and return rows affected.
    pub async fn execute<C: Connection>(
        self,
        cx: &Cx,
        conn: &C,
    ) -> Outcome<u64, sqlmodel_core::Error> {
        let (sql, params) = self.build();
        conn.execute(cx, &sql, &params).await
    }
}

impl<M: Model> Default for DeleteBuilder<M> {
    fn default() -> Self {
        Self::new()
    }
}

/// Query builder for raw SQL with type-safe parameter binding.
#[derive(Debug)]
pub struct QueryBuilder {
    sql: String,
    params: Vec<Value>,
}

impl QueryBuilder {
    /// Create a new query builder with the given SQL.
    pub fn new(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            params: Vec::new(),
        }
    }

    /// Bind a parameter value.
    pub fn bind(mut self, value: impl Into<Value>) -> Self {
        self.params.push(value.into());
        self
    }

    /// Bind multiple parameter values.
    pub fn bind_all(mut self, values: impl IntoIterator<Item = Value>) -> Self {
        self.params.extend(values);
        self
    }

    /// Get the SQL and parameters.
    pub fn build(self) -> (String, Vec<Value>) {
        (self.sql, self.params)
    }
}
