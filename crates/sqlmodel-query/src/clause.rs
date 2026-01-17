//! SQL clause types (WHERE, ORDER BY, LIMIT, etc.)

use crate::expr::Expr;
use sqlmodel_core::Value;

/// WHERE clause.
#[derive(Debug, Clone)]
pub struct Where {
    expr: Expr,
}

impl Where {
    /// Create a new WHERE clause with the given expression.
    pub fn new(expr: Expr) -> Self {
        Self { expr }
    }

    /// Add an AND condition.
    pub fn and(self, expr: Expr) -> Self {
        Self {
            expr: self.expr.and(expr),
        }
    }

    /// Add an OR condition.
    pub fn or(self, expr: Expr) -> Self {
        Self {
            expr: self.expr.or(expr),
        }
    }

    /// Build the WHERE clause SQL and parameters.
    pub fn build(&self) -> (String, Vec<Value>) {
        self.build_with_offset(0)
    }

    /// Build the WHERE clause with a parameter offset.
    pub fn build_with_offset(&self, offset: usize) -> (String, Vec<Value>) {
        let mut params = Vec::new();
        let sql = self.expr.build(&mut params, offset);
        (sql, params)
    }
}

/// ORDER BY clause.
#[derive(Debug, Clone)]
pub struct OrderBy {
    column: String,
    direction: OrderDirection,
    nulls: Option<NullsOrder>,
}

/// Sort direction.
#[derive(Debug, Clone, Copy, Default)]
pub enum OrderDirection {
    #[default]
    Asc,
    Desc,
}

/// NULLS FIRST/LAST ordering.
#[derive(Debug, Clone, Copy)]
pub enum NullsOrder {
    First,
    Last,
}

impl OrderBy {
    /// Create an ascending order by clause.
    pub fn asc(column: impl Into<String>) -> Self {
        Self {
            column: column.into(),
            direction: OrderDirection::Asc,
            nulls: None,
        }
    }

    /// Create a descending order by clause.
    pub fn desc(column: impl Into<String>) -> Self {
        Self {
            column: column.into(),
            direction: OrderDirection::Desc,
            nulls: None,
        }
    }

    /// Set NULLS FIRST.
    pub fn nulls_first(mut self) -> Self {
        self.nulls = Some(NullsOrder::First);
        self
    }

    /// Set NULLS LAST.
    pub fn nulls_last(mut self) -> Self {
        self.nulls = Some(NullsOrder::Last);
        self
    }

    /// Generate SQL for this ORDER BY clause.
    pub fn to_sql(&self) -> String {
        let mut sql = self.column.clone();

        sql.push_str(match self.direction {
            OrderDirection::Asc => " ASC",
            OrderDirection::Desc => " DESC",
        });

        if let Some(nulls) = self.nulls {
            sql.push_str(match nulls {
                NullsOrder::First => " NULLS FIRST",
                NullsOrder::Last => " NULLS LAST",
            });
        }

        sql
    }
}

/// LIMIT clause.
#[derive(Debug, Clone, Copy)]
pub struct Limit(pub u64);

/// OFFSET clause.
#[derive(Debug, Clone, Copy)]
pub struct Offset(pub u64);

/// GROUP BY clause helper.
#[derive(Debug, Clone)]
pub struct GroupBy {
    columns: Vec<String>,
}

impl GroupBy {
    /// Create a new GROUP BY clause.
    pub fn new(columns: &[&str]) -> Self {
        Self {
            columns: columns.iter().map(|&s| s.to_string()).collect(),
        }
    }

    /// Generate SQL for this GROUP BY clause.
    pub fn to_sql(&self) -> String {
        self.columns.join(", ")
    }
}
