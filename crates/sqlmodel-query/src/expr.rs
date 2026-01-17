//! SQL expressions for query building.

use sqlmodel_core::Value;

/// A SQL expression that can be used in WHERE, HAVING, etc.
#[derive(Debug, Clone)]
pub enum Expr {
    /// Column reference
    Column(String),
    /// Literal value
    Literal(Value),
    /// Binary operation (e.g., a = b, a > b)
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    /// Unary operation (e.g., NOT a, -a)
    Unary { op: UnaryOp, expr: Box<Expr> },
    /// Function call
    Function { name: String, args: Vec<Expr> },
    /// IN expression
    In { expr: Box<Expr>, values: Vec<Expr> },
    /// NOT IN expression
    NotIn { expr: Box<Expr>, values: Vec<Expr> },
    /// BETWEEN expression
    Between {
        expr: Box<Expr>,
        low: Box<Expr>,
        high: Box<Expr>,
    },
    /// IS NULL
    IsNull(Box<Expr>),
    /// IS NOT NULL
    IsNotNull(Box<Expr>),
    /// LIKE pattern
    Like { expr: Box<Expr>, pattern: String },
    /// ILIKE pattern (case-insensitive)
    ILike { expr: Box<Expr>, pattern: String },
    /// Subquery
    Subquery(String),
    /// Raw SQL fragment
    Raw(String),
    /// Parenthesized expression
    Paren(Box<Expr>),
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    BitwiseAnd,
    BitwiseOr,
    Concat,
}

impl BinaryOp {
    /// Get the SQL representation of this operator.
    pub const fn as_str(&self) -> &'static str {
        match self {
            BinaryOp::Eq => "=",
            BinaryOp::Ne => "<>",
            BinaryOp::Lt => "<",
            BinaryOp::Le => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::Ge => ">=",
            BinaryOp::And => "AND",
            BinaryOp::Or => "OR",
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Mod => "%",
            BinaryOp::BitwiseAnd => "&",
            BinaryOp::BitwiseOr => "|",
            BinaryOp::Concat => "||",
        }
    }
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
    Neg,
    BitwiseNot,
}

impl UnaryOp {
    /// Get the SQL representation of this operator.
    pub const fn as_str(&self) -> &'static str {
        match self {
            UnaryOp::Not => "NOT",
            UnaryOp::Neg => "-",
            UnaryOp::BitwiseNot => "~",
        }
    }
}

impl Expr {
    /// Create a column reference expression.
    pub fn col(name: impl Into<String>) -> Self {
        Expr::Column(name.into())
    }

    /// Create a literal value expression.
    pub fn lit(value: impl Into<Value>) -> Self {
        Expr::Literal(value.into())
    }

    /// Create a raw SQL expression.
    pub fn raw(sql: impl Into<String>) -> Self {
        Expr::Raw(sql.into())
    }

    // Comparison operators

    /// Equal to
    pub fn eq(self, other: impl Into<Expr>) -> Self {
        Expr::Binary {
            left: Box::new(self),
            op: BinaryOp::Eq,
            right: Box::new(other.into()),
        }
    }

    /// Not equal to
    pub fn ne(self, other: impl Into<Expr>) -> Self {
        Expr::Binary {
            left: Box::new(self),
            op: BinaryOp::Ne,
            right: Box::new(other.into()),
        }
    }

    /// Less than
    pub fn lt(self, other: impl Into<Expr>) -> Self {
        Expr::Binary {
            left: Box::new(self),
            op: BinaryOp::Lt,
            right: Box::new(other.into()),
        }
    }

    /// Less than or equal to
    pub fn le(self, other: impl Into<Expr>) -> Self {
        Expr::Binary {
            left: Box::new(self),
            op: BinaryOp::Le,
            right: Box::new(other.into()),
        }
    }

    /// Greater than
    pub fn gt(self, other: impl Into<Expr>) -> Self {
        Expr::Binary {
            left: Box::new(self),
            op: BinaryOp::Gt,
            right: Box::new(other.into()),
        }
    }

    /// Greater than or equal to
    pub fn ge(self, other: impl Into<Expr>) -> Self {
        Expr::Binary {
            left: Box::new(self),
            op: BinaryOp::Ge,
            right: Box::new(other.into()),
        }
    }

    // Logical operators

    /// Logical AND
    pub fn and(self, other: impl Into<Expr>) -> Self {
        Expr::Binary {
            left: Box::new(self),
            op: BinaryOp::And,
            right: Box::new(other.into()),
        }
    }

    /// Logical OR
    pub fn or(self, other: impl Into<Expr>) -> Self {
        Expr::Binary {
            left: Box::new(self),
            op: BinaryOp::Or,
            right: Box::new(other.into()),
        }
    }

    /// Logical NOT
    pub fn not(self) -> Self {
        Expr::Unary {
            op: UnaryOp::Not,
            expr: Box::new(self),
        }
    }

    // Null checks

    /// IS NULL
    pub fn is_null(self) -> Self {
        Expr::IsNull(Box::new(self))
    }

    /// IS NOT NULL
    pub fn is_not_null(self) -> Self {
        Expr::IsNotNull(Box::new(self))
    }

    // Pattern matching

    /// LIKE pattern match
    pub fn like(self, pattern: impl Into<String>) -> Self {
        Expr::Like {
            expr: Box::new(self),
            pattern: pattern.into(),
        }
    }

    /// ILIKE (case-insensitive) pattern match
    pub fn ilike(self, pattern: impl Into<String>) -> Self {
        Expr::ILike {
            expr: Box::new(self),
            pattern: pattern.into(),
        }
    }

    // IN expressions

    /// IN list of values
    pub fn in_list(self, values: Vec<impl Into<Expr>>) -> Self {
        Expr::In {
            expr: Box::new(self),
            values: values.into_iter().map(Into::into).collect(),
        }
    }

    /// NOT IN list of values
    pub fn not_in_list(self, values: Vec<impl Into<Expr>>) -> Self {
        Expr::NotIn {
            expr: Box::new(self),
            values: values.into_iter().map(Into::into).collect(),
        }
    }

    /// BETWEEN low AND high
    pub fn between(self, low: impl Into<Expr>, high: impl Into<Expr>) -> Self {
        Expr::Between {
            expr: Box::new(self),
            low: Box::new(low.into()),
            high: Box::new(high.into()),
        }
    }

    /// Build SQL string and collect parameters.
    pub fn build(&self, params: &mut Vec<Value>, offset: usize) -> String {
        match self {
            Expr::Column(name) => name.clone(),
            Expr::Literal(value) => {
                params.push(value.clone());
                format!("${}", offset + params.len())
            }
            Expr::Binary { left, op, right } => {
                let left_sql = left.build(params, offset);
                let right_sql = right.build(params, offset);
                format!("{} {} {}", left_sql, op.as_str(), right_sql)
            }
            Expr::Unary { op, expr } => {
                let expr_sql = expr.build(params, offset);
                format!("{} {}", op.as_str(), expr_sql)
            }
            Expr::Function { name, args } => {
                let arg_sqls: Vec<_> = args.iter().map(|a| a.build(params, offset)).collect();
                format!("{}({})", name, arg_sqls.join(", "))
            }
            Expr::In { expr, values } => {
                let expr_sql = expr.build(params, offset);
                let value_sqls: Vec<_> = values.iter().map(|v| v.build(params, offset)).collect();
                format!("{} IN ({})", expr_sql, value_sqls.join(", "))
            }
            Expr::NotIn { expr, values } => {
                let expr_sql = expr.build(params, offset);
                let value_sqls: Vec<_> = values.iter().map(|v| v.build(params, offset)).collect();
                format!("{} NOT IN ({})", expr_sql, value_sqls.join(", "))
            }
            Expr::Between { expr, low, high } => {
                let expr_sql = expr.build(params, offset);
                let low_sql = low.build(params, offset);
                let high_sql = high.build(params, offset);
                format!("{} BETWEEN {} AND {}", expr_sql, low_sql, high_sql)
            }
            Expr::IsNull(expr) => {
                let expr_sql = expr.build(params, offset);
                format!("{} IS NULL", expr_sql)
            }
            Expr::IsNotNull(expr) => {
                let expr_sql = expr.build(params, offset);
                format!("{} IS NOT NULL", expr_sql)
            }
            Expr::Like { expr, pattern } => {
                let expr_sql = expr.build(params, offset);
                params.push(Value::Text(pattern.clone()));
                format!("{} LIKE ${}", expr_sql, offset + params.len())
            }
            Expr::ILike { expr, pattern } => {
                let expr_sql = expr.build(params, offset);
                params.push(Value::Text(pattern.clone()));
                format!("{} ILIKE ${}", expr_sql, offset + params.len())
            }
            Expr::Subquery(sql) => format!("({})", sql),
            Expr::Raw(sql) => sql.clone(),
            Expr::Paren(expr) => {
                let expr_sql = expr.build(params, offset);
                format!("({})", expr_sql)
            }
        }
    }
}

// Conversion from Value to Expr
impl From<Value> for Expr {
    fn from(v: Value) -> Self {
        Expr::Literal(v)
    }
}

impl From<&str> for Expr {
    fn from(s: &str) -> Self {
        Expr::Literal(Value::Text(s.to_string()))
    }
}

impl From<String> for Expr {
    fn from(s: String) -> Self {
        Expr::Literal(Value::Text(s))
    }
}

impl From<i32> for Expr {
    fn from(n: i32) -> Self {
        Expr::Literal(Value::Int(n))
    }
}

impl From<i64> for Expr {
    fn from(n: i64) -> Self {
        Expr::Literal(Value::BigInt(n))
    }
}

impl From<bool> for Expr {
    fn from(b: bool) -> Self {
        Expr::Literal(Value::Bool(b))
    }
}
