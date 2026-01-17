//! SELECT query builder.

use crate::clause::{Limit, Offset, OrderBy, Where};
use crate::expr::Expr;
use crate::join::Join;
use asupersync::{Cx, Outcome};
use sqlmodel_core::{Connection, Model, Value};
use std::marker::PhantomData;

/// A SELECT query builder.
///
/// Provides a fluent API for building SELECT queries with
/// type-safe column references and conditions.
#[derive(Debug, Clone)]
pub struct Select<M: Model> {
    /// Columns to select (empty = all)
    columns: Vec<String>,
    /// WHERE clause conditions
    where_clause: Option<Where>,
    /// ORDER BY clauses
    order_by: Vec<OrderBy>,
    /// JOIN clauses
    joins: Vec<Join>,
    /// LIMIT clause
    limit: Option<Limit>,
    /// OFFSET clause
    offset: Option<Offset>,
    /// GROUP BY columns
    group_by: Vec<String>,
    /// HAVING clause
    having: Option<Where>,
    /// DISTINCT flag
    distinct: bool,
    /// FOR UPDATE flag
    for_update: bool,
    /// Model type marker
    _marker: PhantomData<M>,
}

impl<M: Model> Select<M> {
    /// Create a new SELECT query for the model's table.
    pub fn new() -> Self {
        Self {
            columns: Vec::new(),
            where_clause: None,
            order_by: Vec::new(),
            joins: Vec::new(),
            limit: None,
            offset: None,
            group_by: Vec::new(),
            having: None,
            distinct: false,
            for_update: false,
            _marker: PhantomData,
        }
    }

    /// Select specific columns.
    pub fn columns(mut self, cols: &[&str]) -> Self {
        self.columns = cols.iter().map(|&s| s.to_string()).collect();
        self
    }

    /// Add a WHERE condition.
    pub fn filter(mut self, expr: Expr) -> Self {
        self.where_clause = Some(match self.where_clause {
            Some(existing) => existing.and(expr),
            None => Where::new(expr),
        });
        self
    }

    /// Add an OR WHERE condition.
    pub fn or_filter(mut self, expr: Expr) -> Self {
        self.where_clause = Some(match self.where_clause {
            Some(existing) => existing.or(expr),
            None => Where::new(expr),
        });
        self
    }

    /// Add ORDER BY clause.
    pub fn order_by(mut self, order: OrderBy) -> Self {
        self.order_by.push(order);
        self
    }

    /// Add a JOIN clause.
    pub fn join(mut self, join: Join) -> Self {
        self.joins.push(join);
        self
    }

    /// Set LIMIT.
    pub fn limit(mut self, n: u64) -> Self {
        self.limit = Some(Limit(n));
        self
    }

    /// Set OFFSET.
    pub fn offset(mut self, n: u64) -> Self {
        self.offset = Some(Offset(n));
        self
    }

    /// Add GROUP BY columns.
    pub fn group_by(mut self, cols: &[&str]) -> Self {
        self.group_by.extend(cols.iter().map(|&s| s.to_string()));
        self
    }

    /// Add HAVING condition.
    pub fn having(mut self, expr: Expr) -> Self {
        self.having = Some(match self.having {
            Some(existing) => existing.and(expr),
            None => Where::new(expr),
        });
        self
    }

    /// Make this a DISTINCT query.
    pub fn distinct(mut self) -> Self {
        self.distinct = true;
        self
    }

    /// Add FOR UPDATE lock.
    pub fn for_update(mut self) -> Self {
        self.for_update = true;
        self
    }

    /// Build the SQL query and parameters.
    pub fn build(&self) -> (String, Vec<Value>) {
        let mut sql = String::new();
        let mut params = Vec::new();

        // SELECT
        sql.push_str("SELECT ");
        if self.distinct {
            sql.push_str("DISTINCT ");
        }

        if self.columns.is_empty() {
            sql.push('*');
        } else {
            sql.push_str(&self.columns.join(", "));
        }

        // FROM
        sql.push_str(" FROM ");
        sql.push_str(M::TABLE_NAME);

        // JOINs
        for join in &self.joins {
            sql.push_str(&join.to_sql());
        }

        // WHERE
        if let Some(where_clause) = &self.where_clause {
            let (where_sql, where_params) = where_clause.build();
            sql.push_str(" WHERE ");
            sql.push_str(&where_sql);
            params.extend(where_params);
        }

        // GROUP BY
        if !self.group_by.is_empty() {
            sql.push_str(" GROUP BY ");
            sql.push_str(&self.group_by.join(", "));
        }

        // HAVING
        if let Some(having) = &self.having {
            let (having_sql, having_params) = having.build();
            sql.push_str(" HAVING ");
            sql.push_str(&having_sql);
            params.extend(having_params);
        }

        // ORDER BY
        if !self.order_by.is_empty() {
            sql.push_str(" ORDER BY ");
            let order_strs: Vec<_> = self.order_by.iter().map(|o| o.to_sql()).collect();
            sql.push_str(&order_strs.join(", "));
        }

        // LIMIT
        if let Some(Limit(n)) = self.limit {
            sql.push_str(&format!(" LIMIT {}", n));
        }

        // OFFSET
        if let Some(Offset(n)) = self.offset {
            sql.push_str(&format!(" OFFSET {}", n));
        }

        // FOR UPDATE
        if self.for_update {
            sql.push_str(" FOR UPDATE");
        }

        (sql, params)
    }

    /// Execute the query and return all matching rows as models.
    pub async fn all<C: Connection>(
        self,
        cx: &Cx,
        conn: &C,
    ) -> Outcome<Vec<M>, sqlmodel_core::Error> {
        let (sql, params) = self.build();
        let rows = conn.query(cx, &sql, &params).await;

        rows.and_then(|rows| {
            let mut models = Vec::with_capacity(rows.len());
            for row in &rows {
                match M::from_row(row) {
                    Ok(model) => models.push(model),
                    Err(e) => return Outcome::Err(e),
                }
            }
            Outcome::Ok(models)
        })
    }

    /// Execute the query and return the first matching row.
    pub async fn first<C: Connection>(
        self,
        cx: &Cx,
        conn: &C,
    ) -> Outcome<Option<M>, sqlmodel_core::Error> {
        let query = self.limit(1);
        let (sql, params) = query.build();
        let row = conn.query_one(cx, &sql, &params).await;

        row.and_then(|opt_row| match opt_row {
            Some(row) => match M::from_row(&row) {
                Ok(model) => Outcome::Ok(Some(model)),
                Err(e) => Outcome::Err(e),
            },
            None => Outcome::Ok(None),
        })
    }

    /// Execute the query and return exactly one row, or error.
    pub async fn one<C: Connection>(self, cx: &Cx, conn: &C) -> Outcome<M, sqlmodel_core::Error> {
        match self.first(cx, conn).await {
            Outcome::Ok(Some(model)) => Outcome::Ok(model),
            Outcome::Ok(None) => Outcome::Err(sqlmodel_core::Error::Custom(
                "Expected one row, found none".to_string(),
            )),
            Outcome::Err(e) => Outcome::Err(e),
            Outcome::Cancelled(r) => Outcome::Cancelled(r),
            Outcome::Panicked(p) => Outcome::Panicked(p),
        }
    }

    /// Execute the query and return the count of matching rows.
    pub async fn count<C: Connection>(
        self,
        cx: &Cx,
        conn: &C,
    ) -> Outcome<u64, sqlmodel_core::Error> {
        let mut count_query = self;
        count_query.columns = vec!["COUNT(*) as count".to_string()];
        count_query.order_by.clear();
        count_query.limit = None;
        count_query.offset = None;

        let (sql, params) = count_query.build();
        let row = conn.query_one(cx, &sql, &params).await;

        row.and_then(|opt_row| match opt_row {
            Some(row) => match row.get_named::<i64>("count") {
                Ok(count) => Outcome::Ok(count as u64),
                Err(e) => Outcome::Err(e),
            },
            None => Outcome::Ok(0),
        })
    }

    /// Check if any rows match the query.
    pub async fn exists<C: Connection>(
        self,
        cx: &Cx,
        conn: &C,
    ) -> Outcome<bool, sqlmodel_core::Error> {
        let count = self.count(cx, conn).await;
        count.map(|n| n > 0)
    }
}

impl<M: Model> Default for Select<M> {
    fn default() -> Self {
        Self::new()
    }
}
