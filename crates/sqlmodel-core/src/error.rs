//! Error types for SQLModel operations.

use std::fmt;

/// The primary error type for all SQLModel operations.
#[derive(Debug)]
pub enum Error {
    /// Connection-related errors (connect, disconnect, timeout)
    Connection(ConnectionError),
    /// Query execution errors
    Query(QueryError),
    /// Type conversion errors
    Type(TypeError),
    /// Transaction errors
    Transaction(TransactionError),
    /// Pool errors
    Pool(PoolError),
    /// Schema/migration errors
    Schema(SchemaError),
    /// Serialization/deserialization errors
    Serde(String),
    /// Operation was cancelled via asupersync
    Cancelled,
    /// Custom error with message
    Custom(String),
}

#[derive(Debug)]
pub struct ConnectionError {
    pub kind: ConnectionErrorKind,
    pub message: String,
}

#[derive(Debug)]
pub enum ConnectionErrorKind {
    /// Failed to establish connection
    Connect,
    /// Connection was closed unexpectedly
    Closed,
    /// Connection timeout
    Timeout,
    /// Authentication failed
    Auth,
    /// SSL/TLS error
    Tls,
}

#[derive(Debug)]
pub struct QueryError {
    pub kind: QueryErrorKind,
    pub message: String,
    pub sql: Option<String>,
}

#[derive(Debug)]
pub enum QueryErrorKind {
    /// Syntax error in SQL
    Syntax,
    /// Constraint violation (unique, foreign key, etc.)
    Constraint,
    /// Table or column not found
    NotFound,
    /// Permission denied
    Permission,
    /// Query timeout
    Timeout,
    /// Other execution error
    Execution,
}

#[derive(Debug)]
pub struct TypeError {
    pub expected: String,
    pub found: String,
    pub column: Option<String>,
}

#[derive(Debug)]
pub struct TransactionError {
    pub kind: TransactionErrorKind,
    pub message: String,
}

#[derive(Debug)]
pub enum TransactionErrorKind {
    /// Transaction already started
    AlreadyStarted,
    /// No active transaction
    NoTransaction,
    /// Commit failed
    CommitFailed,
    /// Rollback failed
    RollbackFailed,
    /// Deadlock detected
    Deadlock,
    /// Serialization failure
    Serialization,
}

#[derive(Debug)]
pub struct PoolError {
    pub kind: PoolErrorKind,
    pub message: String,
}

#[derive(Debug)]
pub enum PoolErrorKind {
    /// Pool exhausted (no available connections)
    Exhausted,
    /// Connection checkout timeout
    Timeout,
    /// Pool is closed
    Closed,
    /// Configuration error
    Config,
}

#[derive(Debug)]
pub struct SchemaError {
    pub kind: SchemaErrorKind,
    pub message: String,
}

#[derive(Debug)]
pub enum SchemaErrorKind {
    /// Table already exists
    TableExists,
    /// Table not found
    TableNotFound,
    /// Column already exists
    ColumnExists,
    /// Column not found
    ColumnNotFound,
    /// Invalid schema definition
    Invalid,
    /// Migration error
    Migration,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Connection(e) => write!(f, "Connection error: {}", e.message),
            Error::Query(e) => {
                if let Some(sql) = &e.sql {
                    write!(f, "Query error: {} (SQL: {})", e.message, sql)
                } else {
                    write!(f, "Query error: {}", e.message)
                }
            }
            Error::Type(e) => {
                if let Some(col) = &e.column {
                    write!(
                        f,
                        "Type error in column '{}': expected {}, found {}",
                        col, e.expected, e.found
                    )
                } else {
                    write!(f, "Type error: expected {}, found {}", e.expected, e.found)
                }
            }
            Error::Transaction(e) => write!(f, "Transaction error: {}", e.message),
            Error::Pool(e) => write!(f, "Pool error: {}", e.message),
            Error::Schema(e) => write!(f, "Schema error: {}", e.message),
            Error::Serde(msg) => write!(f, "Serialization error: {}", msg),
            Error::Cancelled => write!(f, "Operation cancelled"),
            Error::Custom(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for Error {}

/// Result type alias for SQLModel operations.
pub type Result<T> = std::result::Result<T, Error>;
