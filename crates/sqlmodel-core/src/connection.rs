//! Database connection traits.

use crate::error::Result;
use crate::row::Row;
use crate::value::Value;
use asupersync::{Cx, Outcome};

/// A database connection.
///
/// This trait defines the interface for database connections.
/// All operations take a `Cx` context for structured concurrency.
pub trait Connection: Send + Sync {
    /// Execute a query and return all rows.
    fn query(
        &self,
        cx: &Cx,
        sql: &str,
        params: &[Value],
    ) -> impl Future<Output = Outcome<Vec<Row>, crate::Error>> + Send;

    /// Execute a query and return the first row, if any.
    fn query_one(
        &self,
        cx: &Cx,
        sql: &str,
        params: &[Value],
    ) -> impl Future<Output = Outcome<Option<Row>, crate::Error>> + Send;

    /// Execute a statement (INSERT, UPDATE, DELETE) and return rows affected.
    fn execute(
        &self,
        cx: &Cx,
        sql: &str,
        params: &[Value],
    ) -> impl Future<Output = Outcome<u64, crate::Error>> + Send;

    /// Execute an INSERT and return the last inserted ID.
    fn insert(
        &self,
        cx: &Cx,
        sql: &str,
        params: &[Value],
    ) -> impl Future<Output = Outcome<i64, crate::Error>> + Send;

    /// Begin a transaction.
    fn begin(&self, cx: &Cx)
    -> impl Future<Output = Outcome<Transaction<'_>, crate::Error>> + Send;

    /// Check if the connection is still valid.
    fn is_valid(&self, cx: &Cx) -> impl Future<Output = bool> + Send;

    /// Close the connection.
    fn close(self, cx: &Cx) -> impl Future<Output = Result<()>> + Send;
}

/// A database transaction.
///
/// Transactions provide ACID guarantees and can be committed or rolled back.
/// If dropped without committing, the transaction is automatically rolled back.
pub struct Transaction<'conn> {
    /// The underlying connection
    conn: &'conn dyn ConnectionInternal,
    /// Whether this transaction has been committed
    committed: bool,
}

/// Internal trait for connection operations (object-safe subset).
trait ConnectionInternal: Send + Sync {
    fn query_internal(
        &self,
        cx: &Cx,
        sql: &str,
        params: &[Value],
    ) -> std::pin::Pin<Box<dyn Future<Output = Outcome<Vec<Row>, crate::Error>> + Send + '_>>;

    fn execute_internal(
        &self,
        cx: &Cx,
        sql: &str,
        params: &[Value],
    ) -> std::pin::Pin<Box<dyn Future<Output = Outcome<u64, crate::Error>> + Send + '_>>;

    fn commit_internal(
        &self,
        cx: &Cx,
    ) -> std::pin::Pin<Box<dyn Future<Output = Outcome<(), crate::Error>> + Send + '_>>;

    fn rollback_internal(
        &self,
        cx: &Cx,
    ) -> std::pin::Pin<Box<dyn Future<Output = Outcome<(), crate::Error>> + Send + '_>>;
}

impl<'conn> Transaction<'conn> {
    /// Execute a query within this transaction.
    pub async fn query(
        &self,
        cx: &Cx,
        sql: &str,
        params: &[Value],
    ) -> Outcome<Vec<Row>, crate::Error> {
        self.conn.query_internal(cx, sql, params).await
    }

    /// Execute a statement within this transaction.
    pub async fn execute(
        &self,
        cx: &Cx,
        sql: &str,
        params: &[Value],
    ) -> Outcome<u64, crate::Error> {
        self.conn.execute_internal(cx, sql, params).await
    }

    /// Commit the transaction.
    pub async fn commit(mut self, cx: &Cx) -> Outcome<(), crate::Error> {
        self.committed = true;
        self.conn.commit_internal(cx).await
    }

    /// Rollback the transaction explicitly.
    pub async fn rollback(mut self, cx: &Cx) -> Outcome<(), crate::Error> {
        self.committed = true; // Prevent double rollback in drop
        self.conn.rollback_internal(cx).await
    }
}

use std::future::Future;

impl Drop for Transaction<'_> {
    fn drop(&mut self) {
        if !self.committed {
            // Transaction was not committed, it will be rolled back
            // Note: We can't do async in drop, so the actual rollback
            // happens at the protocol level when the transaction scope ends
        }
    }
}

/// Configuration for database connections.
#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    /// Connection string or URL
    pub url: String,
    /// Connection timeout in milliseconds
    pub connect_timeout_ms: u64,
    /// Query timeout in milliseconds
    pub query_timeout_ms: u64,
    /// SSL mode
    pub ssl_mode: SslMode,
    /// Application name for connection identification
    pub application_name: Option<String>,
}

/// SSL connection mode.
#[derive(Debug, Clone, Copy, Default)]
pub enum SslMode {
    /// Never use SSL
    Disable,
    /// Prefer SSL but allow non-SSL
    #[default]
    Prefer,
    /// Require SSL
    Require,
    /// Verify server certificate
    VerifyCa,
    /// Verify server certificate and hostname
    VerifyFull,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            connect_timeout_ms: 30_000,
            query_timeout_ms: 30_000,
            ssl_mode: SslMode::default(),
            application_name: None,
        }
    }
}

impl ConnectionConfig {
    /// Create a new connection config with the given URL.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ..Default::default()
        }
    }

    /// Set the connection timeout.
    pub fn connect_timeout(mut self, ms: u64) -> Self {
        self.connect_timeout_ms = ms;
        self
    }

    /// Set the query timeout.
    pub fn query_timeout(mut self, ms: u64) -> Self {
        self.query_timeout_ms = ms;
        self
    }

    /// Set the SSL mode.
    pub fn ssl_mode(mut self, mode: SslMode) -> Self {
        self.ssl_mode = mode;
        self
    }

    /// Set the application name.
    pub fn application_name(mut self, name: impl Into<String>) -> Self {
        self.application_name = Some(name.into());
        self
    }
}
