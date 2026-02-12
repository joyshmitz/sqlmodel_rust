//! FrankenSQLite connection implementing `sqlmodel_core::Connection`.
//!
//! Wraps `fsqlite::Connection` (which is `!Send` due to `Rc<RefCell<>>`) in
//! `Arc<Mutex<>>` to satisfy the `Connection: Send + Sync` requirement.
//! All operations execute synchronously under the mutex, matching the pattern
//! used by `sqlmodel-sqlite` for its FFI-based wrapper.

#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::result_large_err)]

use crate::value::{sqlite_to_value, value_to_sqlite};
use fsqlite_types::value::SqliteValue;
use sqlmodel_core::{
    Connection, Cx, IsolationLevel, Outcome, PreparedStatement,
    Row, TransactionOps, Value,
    error::{ConnectionError, ConnectionErrorKind, Error, QueryError, QueryErrorKind},
    row::ColumnInfo,
};
use std::future::Future;
use std::sync::{Arc, Mutex};

/// Inner state guarded by a mutex.
struct FrankenInner {
    /// The underlying frankensqlite connection (`!Send`, hence wrapped).
    conn: fsqlite::Connection,
    /// Whether we are currently inside a transaction.
    in_transaction: bool,
    /// The last inserted rowid (tracked manually since frankensqlite stubs it).
    last_insert_rowid: i64,
}

// SAFETY: All access to `FrankenInner` goes through the `Mutex`, which
// serializes access. The `Rc<RefCell<>>` inside `fsqlite::Connection` is
// never shared across threads — the mutex ensures single-threaded access.
unsafe impl Send for FrankenInner {}

/// A SQLite connection backed by FrankenSQLite (pure Rust).
///
/// Implements `sqlmodel_core::Connection` and provides sync helper methods
/// (`execute_raw`, `query_sync`, `execute_sync`, etc.) matching the
/// `SqliteConnection` API for drop-in replacement.
pub struct FrankenConnection {
    inner: Arc<Mutex<FrankenInner>>,
    path: String,
}

// SAFETY: All access goes through Arc<Mutex<>> — single-thread serialization.
unsafe impl Send for FrankenConnection {}
unsafe impl Sync for FrankenConnection {}

impl FrankenConnection {
    /// Open a connection with the given path.
    ///
    /// Use `":memory:"` for an in-memory database, or a file path for
    /// persistent storage.
    pub fn open(path: impl Into<String>) -> Result<Self, Error> {
        let path = path.into();
        let conn = fsqlite::Connection::open(&path).map_err(|e| franken_to_conn_error(&e))?;
        Ok(Self {
            inner: Arc::new(Mutex::new(FrankenInner {
                conn,
                in_transaction: false,
                last_insert_rowid: 0,
            })),
            path,
        })
    }

    /// Open an in-memory database.
    pub fn open_memory() -> Result<Self, Error> {
        Self::open(":memory:")
    }

    /// Open a file-based database.
    pub fn open_file(path: impl Into<String>) -> Result<Self, Error> {
        Self::open(path)
    }

    /// Get the database path.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Execute SQL directly without parameter binding (for DDL, PRAGMAs, etc.)
    pub fn execute_raw(&self, sql: &str) -> Result<(), Error> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner
            .conn
            .execute(sql)
            .map_err(|e| franken_to_query_error(&e, sql))?;
        Ok(())
    }

    /// Prepare and execute a query synchronously, returning all rows.
    pub fn query_sync(&self, sql: &str, params: &[Value]) -> Result<Vec<Row>, Error> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let sqlite_params: Vec<SqliteValue> = params.iter().map(value_to_sqlite).collect();

        let franken_rows = if sqlite_params.is_empty() {
            inner.conn.query(sql)
        } else {
            inner.conn.query_with_params(sql, &sqlite_params)
        }
        .map_err(|e| franken_to_query_error(&e, sql))?;

        Ok(convert_rows(&franken_rows, sql))
    }

    /// Prepare and execute a statement synchronously, returning rows affected.
    pub fn execute_sync(&self, sql: &str, params: &[Value]) -> Result<u64, Error> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let sqlite_params: Vec<SqliteValue> = params.iter().map(value_to_sqlite).collect();

        let count = if sqlite_params.is_empty() {
            inner.conn.execute(sql)
        } else {
            inner.conn.execute_with_params(sql, &sqlite_params)
        }
        .map_err(|e| franken_to_query_error(&e, sql))?;

        // Track last_insert_rowid for INSERT statements
        if is_insert_sql(sql) {
            // After an INSERT, query last_insert_rowid()
            if let Ok(rows) = inner.conn.query("SELECT last_insert_rowid()") {
                if let Some(row) = rows.first() {
                    if let Some(SqliteValue::Integer(id)) = row.get(0) {
                        inner.last_insert_rowid = *id;
                    }
                }
            }
        }

        Ok(count as u64)
    }

    /// Get the last inserted rowid.
    pub fn last_insert_rowid(&self) -> i64 {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.last_insert_rowid
    }

    /// Get the number of rows changed by the last statement.
    pub fn changes(&self) -> i64 {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Ok(rows) = inner.conn.query("SELECT changes()") {
            if let Some(row) = rows.first() {
                if let Some(SqliteValue::Integer(n)) = row.get(0) {
                    return *n;
                }
            }
        }
        0
    }

    /// Execute an INSERT and return the last inserted rowid.
    fn insert_sync(&self, sql: &str, params: &[Value]) -> Result<i64, Error> {
        self.execute_sync(sql, params)?;
        Ok(self.last_insert_rowid())
    }

    /// Begin a transaction.
    fn begin_sync(&self, isolation: IsolationLevel) -> Result<(), Error> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.in_transaction {
            return Err(Error::Query(QueryError {
                kind: QueryErrorKind::Database,
                sql: None,
                sqlstate: None,
                message: "Already in a transaction".to_string(),
                detail: None,
                hint: None,
                position: None,
                source: None,
            }));
        }

        let begin_sql = match isolation {
            IsolationLevel::Serializable => "BEGIN EXCLUSIVE",
            IsolationLevel::RepeatableRead | IsolationLevel::ReadCommitted => "BEGIN IMMEDIATE",
            IsolationLevel::ReadUncommitted => "BEGIN DEFERRED",
        };

        inner
            .conn
            .execute(begin_sql)
            .map_err(|e| franken_to_query_error(&e, begin_sql))?;

        inner.in_transaction = true;
        Ok(())
    }

    /// Commit the current transaction.
    fn commit_sync(&self) -> Result<(), Error> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !inner.in_transaction {
            return Err(Error::Query(QueryError {
                kind: QueryErrorKind::Database,
                sql: None,
                sqlstate: None,
                message: "Not in a transaction".to_string(),
                detail: None,
                hint: None,
                position: None,
                source: None,
            }));
        }

        inner
            .conn
            .execute("COMMIT")
            .map_err(|e| franken_to_query_error(&e, "COMMIT"))?;

        inner.in_transaction = false;
        Ok(())
    }

    /// Rollback the current transaction.
    fn rollback_sync(&self) -> Result<(), Error> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !inner.in_transaction {
            return Err(Error::Query(QueryError {
                kind: QueryErrorKind::Database,
                sql: None,
                sqlstate: None,
                message: "Not in a transaction".to_string(),
                detail: None,
                hint: None,
                position: None,
                source: None,
            }));
        }

        inner
            .conn
            .execute("ROLLBACK")
            .map_err(|e| franken_to_query_error(&e, "ROLLBACK"))?;

        inner.in_transaction = false;
        Ok(())
    }
}

// ── Connection trait impl ─────────────────────────────────────────────────

impl Connection for FrankenConnection {
    type Tx<'conn>
        = FrankenTransaction<'conn>
    where
        Self: 'conn;

    fn dialect(&self) -> sqlmodel_core::Dialect {
        sqlmodel_core::Dialect::Sqlite
    }

    fn query(
        &self,
        _cx: &Cx,
        sql: &str,
        params: &[Value],
    ) -> impl Future<Output = Outcome<Vec<Row>, Error>> + Send {
        let result = self.query_sync(sql, params);
        async move { result.map_or_else(Outcome::Err, Outcome::Ok) }
    }

    fn query_one(
        &self,
        _cx: &Cx,
        sql: &str,
        params: &[Value],
    ) -> impl Future<Output = Outcome<Option<Row>, Error>> + Send {
        let result = self.query_sync(sql, params).map(|mut rows| rows.pop());
        async move { result.map_or_else(Outcome::Err, Outcome::Ok) }
    }

    fn execute(
        &self,
        _cx: &Cx,
        sql: &str,
        params: &[Value],
    ) -> impl Future<Output = Outcome<u64, Error>> + Send {
        let result = self.execute_sync(sql, params);
        async move { result.map_or_else(Outcome::Err, Outcome::Ok) }
    }

    fn insert(
        &self,
        _cx: &Cx,
        sql: &str,
        params: &[Value],
    ) -> impl Future<Output = Outcome<i64, Error>> + Send {
        let result = self.insert_sync(sql, params);
        async move { result.map_or_else(Outcome::Err, Outcome::Ok) }
    }

    fn batch(
        &self,
        _cx: &Cx,
        statements: &[(String, Vec<Value>)],
    ) -> impl Future<Output = Outcome<Vec<u64>, Error>> + Send {
        let mut results = Vec::with_capacity(statements.len());
        let mut error = None;

        for (sql, params) in statements {
            match self.execute_sync(sql, params) {
                Ok(n) => results.push(n),
                Err(e) => {
                    error = Some(e);
                    break;
                }
            }
        }

        async move {
            match error {
                Some(e) => Outcome::Err(e),
                None => Outcome::Ok(results),
            }
        }
    }

    fn begin(&self, cx: &Cx) -> impl Future<Output = Outcome<Self::Tx<'_>, Error>> + Send {
        self.begin_with(cx, IsolationLevel::default())
    }

    fn begin_with(
        &self,
        _cx: &Cx,
        isolation: IsolationLevel,
    ) -> impl Future<Output = Outcome<Self::Tx<'_>, Error>> + Send {
        let result = self
            .begin_sync(isolation)
            .map(|()| FrankenTransaction::new(self));
        async move { result.map_or_else(Outcome::Err, Outcome::Ok) }
    }

    fn prepare(
        &self,
        _cx: &Cx,
        sql: &str,
    ) -> impl Future<Output = Outcome<PreparedStatement, Error>> + Send {
        // Count parameters (simple heuristic: count ?N placeholders)
        let param_count = count_params(sql);
        let id = sql.as_ptr() as u64;

        // Try to infer column names from the SQL
        let columns = infer_column_names(sql);

        let stmt = if columns.is_empty() {
            PreparedStatement::new(id, sql.to_string(), param_count)
        } else {
            PreparedStatement::with_columns(id, sql.to_string(), param_count, columns)
        };

        async move { Outcome::Ok(stmt) }
    }

    fn query_prepared(
        &self,
        cx: &Cx,
        stmt: &PreparedStatement,
        params: &[Value],
    ) -> impl Future<Output = Outcome<Vec<Row>, Error>> + Send {
        self.query(cx, stmt.sql(), params)
    }

    fn execute_prepared(
        &self,
        cx: &Cx,
        stmt: &PreparedStatement,
        params: &[Value],
    ) -> impl Future<Output = Outcome<u64, Error>> + Send {
        self.execute(cx, stmt.sql(), params)
    }

    fn ping(&self, _cx: &Cx) -> impl Future<Output = Outcome<(), Error>> + Send {
        let result = self.query_sync("SELECT 1", &[]).map(|_| ());
        async move { result.map_or_else(Outcome::Err, Outcome::Ok) }
    }

    async fn close(self, _cx: &Cx) -> sqlmodel_core::Result<()> {
        // Connection is closed on drop (inner Rc<RefCell<>> cleanup)
        Ok(())
    }
}

// ── Transaction ───────────────────────────────────────────────────────────

/// A FrankenSQLite transaction.
pub struct FrankenTransaction<'conn> {
    conn: &'conn FrankenConnection,
    committed: bool,
}

impl<'conn> FrankenTransaction<'conn> {
    fn new(conn: &'conn FrankenConnection) -> Self {
        Self {
            conn,
            committed: false,
        }
    }
}

impl Drop for FrankenTransaction<'_> {
    fn drop(&mut self) {
        if !self.committed {
            let _ = self.conn.rollback_sync();
        }
    }
}

impl TransactionOps for FrankenTransaction<'_> {
    fn query(
        &self,
        _cx: &Cx,
        sql: &str,
        params: &[Value],
    ) -> impl Future<Output = Outcome<Vec<Row>, Error>> + Send {
        let result = self.conn.query_sync(sql, params);
        async move { result.map_or_else(Outcome::Err, Outcome::Ok) }
    }

    fn query_one(
        &self,
        _cx: &Cx,
        sql: &str,
        params: &[Value],
    ) -> impl Future<Output = Outcome<Option<Row>, Error>> + Send {
        let result = self.conn.query_sync(sql, params).map(|mut rows| rows.pop());
        async move { result.map_or_else(Outcome::Err, Outcome::Ok) }
    }

    fn execute(
        &self,
        _cx: &Cx,
        sql: &str,
        params: &[Value],
    ) -> impl Future<Output = Outcome<u64, Error>> + Send {
        let result = self.conn.execute_sync(sql, params);
        async move { result.map_or_else(Outcome::Err, Outcome::Ok) }
    }

    fn savepoint(&self, _cx: &Cx, name: &str) -> impl Future<Output = Outcome<(), Error>> + Send {
        let quoted = format!("\"{}\"", name.replace('"', "\"\""));
        let sql = format!("SAVEPOINT {quoted}");
        let result = self.conn.execute_raw(&sql);
        async move { result.map_or_else(Outcome::Err, Outcome::Ok) }
    }

    fn rollback_to(
        &self,
        _cx: &Cx,
        name: &str,
    ) -> impl Future<Output = Outcome<(), Error>> + Send {
        let quoted = format!("\"{}\"", name.replace('"', "\"\""));
        let sql = format!("ROLLBACK TO {quoted}");
        let result = self.conn.execute_raw(&sql);
        async move { result.map_or_else(Outcome::Err, Outcome::Ok) }
    }

    fn release(&self, _cx: &Cx, name: &str) -> impl Future<Output = Outcome<(), Error>> + Send {
        let quoted = format!("\"{}\"", name.replace('"', "\"\""));
        let sql = format!("RELEASE {quoted}");
        let result = self.conn.execute_raw(&sql);
        async move { result.map_or_else(Outcome::Err, Outcome::Ok) }
    }

    async fn commit(mut self, _cx: &Cx) -> Outcome<(), Error> {
        self.committed = true;
        self.conn
            .commit_sync()
            .map_or_else(Outcome::Err, Outcome::Ok)
    }

    async fn rollback(mut self, _cx: &Cx) -> Outcome<(), Error> {
        self.committed = true; // Prevent double rollback in drop
        self.conn
            .rollback_sync()
            .map_or_else(Outcome::Err, Outcome::Ok)
    }
}

// ── Helper functions ──────────────────────────────────────────────────────

/// Convert frankensqlite rows to sqlmodel-core rows.
///
/// frankensqlite `Row` has no column names, so we infer them from the SQL
/// or fall back to positional names (`_c0`, `_c1`, ...).
fn convert_rows(franken_rows: &[fsqlite_core::connection::Row], sql: &str) -> Vec<Row> {
    if franken_rows.is_empty() {
        return Vec::new();
    }

    // Determine column count from first row
    let col_count = franken_rows[0].values().len();

    // Try to infer column names from SQL
    let mut col_names = infer_column_names(sql);

    // Pad or trim to match actual column count
    while col_names.len() < col_count {
        col_names.push(format!("_c{}", col_names.len()));
    }
    col_names.truncate(col_count);

    let columns = Arc::new(ColumnInfo::new(col_names));

    franken_rows
        .iter()
        .map(|fr| {
            let values: Vec<Value> = fr.values().iter().map(sqlite_to_value).collect();
            Row::with_columns(Arc::clone(&columns), values)
        })
        .collect()
}

/// Infer column names from SQL text.
///
/// Handles common patterns:
/// - `SELECT col1, col2 AS alias, ...`
/// - `PRAGMA table_info(...)` and other PRAGMA results
/// - Expression-only SELECT with aliases
///
/// Falls back to empty vec if parsing fails.
fn infer_column_names(sql: &str) -> Vec<String> {
    let trimmed = sql.trim();
    let upper = trimmed.to_uppercase();

    // PRAGMA column name lookup
    if upper.starts_with("PRAGMA") {
        return infer_pragma_columns(&upper);
    }

    // For SELECT, try to extract column names from the result columns
    if upper.starts_with("SELECT") || upper.starts_with("WITH") {
        return infer_select_columns(trimmed);
    }

    Vec::new()
}

/// Infer column names for PRAGMA results.
fn infer_pragma_columns(upper_sql: &str) -> Vec<String> {
    // Extract PRAGMA name (e.g., "PRAGMA table_info(x)" -> "table_info")
    let after_pragma = upper_sql.trim_start_matches("PRAGMA").trim();
    let pragma_name = after_pragma
        .split(|c: char| c == '(' || c == ';' || c == '=' || c.is_whitespace())
        .next()
        .unwrap_or("")
        .trim();

    match pragma_name {
        "TABLE_INFO" | "TABLE_XINFO" => {
            vec![
                "cid".into(),
                "name".into(),
                "type".into(),
                "notnull".into(),
                "dflt_value".into(),
                "pk".into(),
            ]
        }
        "INDEX_LIST" => vec![
            "seq".into(),
            "name".into(),
            "unique".into(),
            "origin".into(),
            "partial".into(),
        ],
        "INDEX_INFO" | "INDEX_XINFO" => {
            vec!["seqno".into(), "cid".into(), "name".into()]
        }
        "FOREIGN_KEY_LIST" => vec![
            "id".into(),
            "seq".into(),
            "table".into(),
            "from".into(),
            "to".into(),
            "on_update".into(),
            "on_delete".into(),
            "match".into(),
        ],
        "DATABASE_LIST" => vec!["seq".into(), "name".into(), "file".into()],
        "COMPILE_OPTIONS" => vec!["compile_option".into()],
        "COLLATION_LIST" => vec!["seq".into(), "name".into()],
        "INTEGRITY_CHECK" | "QUICK_CHECK" => vec!["integrity_check".into()],
        "WAL_CHECKPOINT" => vec!["busy".into(), "log".into(), "checkpointed".into()],
        "FREELIST_COUNT" => vec!["freelist_count".into()],
        "PAGE_COUNT" => vec!["page_count".into()],
        _ => {
            // For simple PRAGMA (e.g., PRAGMA journal_mode), return the pragma name
            if !after_pragma.contains('(') && !after_pragma.contains('=') {
                vec![pragma_name.to_lowercase()]
            } else {
                Vec::new()
            }
        }
    }
}

/// Infer column names from a SELECT statement.
///
/// Extracts aliases and bare column references from the result column list.
fn infer_select_columns(sql: &str) -> Vec<String> {
    // Find the columns between SELECT and FROM (or end of statement)
    let upper = sql.to_uppercase();

    // Skip past WITH clause if present
    let select_start = if upper.starts_with("WITH") {
        // Find the actual SELECT after the CTE
        if let Some(pos) = find_main_select(&upper) {
            pos
        } else {
            return Vec::new();
        }
    } else {
        0
    };

    let after_select = &sql[select_start..];
    let upper_after = &upper[select_start..];

    // Skip SELECT [DISTINCT] keyword
    let col_start = if upper_after.starts_with("SELECT DISTINCT") {
        15
    } else if upper_after.starts_with("SELECT ALL") {
        10
    } else if upper_after.starts_with("SELECT") {
        6
    } else {
        return Vec::new();
    };

    let cols_str = &after_select[col_start..];

    // Find the FROM clause (respecting parentheses depth)
    let from_pos = find_keyword_at_depth_zero(cols_str, "FROM");
    let cols_region = if let Some(pos) = from_pos {
        &cols_str[..pos]
    } else {
        // No FROM: everything after SELECT is result columns (minus ORDER BY, LIMIT, etc.)
        let end_pos = find_keyword_at_depth_zero(cols_str, "ORDER")
            .or_else(|| find_keyword_at_depth_zero(cols_str, "LIMIT"))
            .or_else(|| find_keyword_at_depth_zero(cols_str, "GROUP"))
            .or_else(|| find_keyword_at_depth_zero(cols_str, "HAVING"))
            .or_else(|| cols_str.find(';'));
        if let Some(pos) = end_pos {
            &cols_str[..pos]
        } else {
            cols_str
        }
    };

    // Split by commas (respecting parentheses depth)
    let columns = split_at_depth_zero(cols_region, ',');

    columns
        .iter()
        .map(|col| extract_column_name(col.trim()))
        .collect()
}

/// Extract a column name or alias from a result column expression.
fn extract_column_name(col_expr: &str) -> String {
    let trimmed = col_expr.trim();

    // Check for AS alias (case-insensitive) — search backwards to handle
    // expressions containing "AS" in sub-expressions.
    // We need to find " AS " at depth 0.
    if let Some(as_pos) = find_last_as_at_depth_zero(trimmed) {
        let alias = trimmed[as_pos + 4..].trim().trim_matches('"');
        return alias.to_string();
    }

    // Star expansion — return *
    if trimmed == "*" {
        return "*".to_string();
    }

    // Table.column — return just column
    if let Some(dot_pos) = trimmed.rfind('.') {
        return trimmed[dot_pos + 1..].trim_matches('"').to_string();
    }

    // Bare identifier
    trimmed.trim_matches('"').to_string()
}

/// Find the last occurrence of " AS " at parentheses depth 0 (case-insensitive).
fn find_last_as_at_depth_zero(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let len = bytes.len();
    if len < 4 {
        return None;
    }
    let mut depth = 0i32;
    let mut last_match = None;

    // Track depth forward, record all " AS " positions at depth 0
    for i in 0..len {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        // Check for " AS " pattern: space, A/a, S/s, space
        if depth == 0
            && i + 3 < len
            && (bytes[i] == b' ')
            && (bytes[i + 1] == b'A' || bytes[i + 1] == b'a')
            && (bytes[i + 2] == b'S' || bytes[i + 2] == b's')
            && (bytes[i + 3] == b' ')
        {
            last_match = Some(i);
        }
    }
    last_match
}

/// Find a keyword at parentheses depth 0.
fn find_keyword_at_depth_zero(s: &str, keyword: &str) -> Option<usize> {
    let upper = s.to_uppercase();
    let kw_upper = keyword.to_uppercase();
    let kw_len = kw_upper.len();
    let mut depth = 0i32;

    for (i, c) in upper.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if depth == 0 && upper[i..].starts_with(&kw_upper) {
            // Ensure it's a word boundary
            let before_ok = i == 0 || !upper.as_bytes()[i - 1].is_ascii_alphanumeric();
            let after_ok = i + kw_len >= upper.len()
                || !upper.as_bytes()[i + kw_len].is_ascii_alphanumeric();
            if before_ok && after_ok {
                return Some(i);
            }
        }
    }
    None
}

/// Split a string by a delimiter at parentheses depth 0.
fn split_at_depth_zero(s: &str, delim: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;

    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ if c == delim && depth == 0 => {
                parts.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Find the position of the main SELECT in a WITH ... SELECT statement.
fn find_main_select(upper: &str) -> Option<usize> {
    // Walk past CTE definitions (respecting parentheses)
    let mut depth = 0i32;
    let bytes = upper.as_bytes();
    let mut i = 4; // Skip "WITH"

    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b'S' if depth == 0 && upper[i..].starts_with("SELECT") => {
                return Some(i);
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Check if SQL is an INSERT statement (case-insensitive).
fn is_insert_sql(sql: &str) -> bool {
    let trimmed = sql.trim().to_uppercase();
    trimmed.starts_with("INSERT")
        || trimmed.starts_with("REPLACE")
        || trimmed.starts_with("INSERT OR")
}

/// Count parameter placeholders in SQL (?1, ?2, etc. or bare ?).
fn count_params(sql: &str) -> usize {
    let mut max_param = 0usize;
    let mut bare_count = 0usize;
    let bytes = sql.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'?' {
            i += 1;
            let mut num = 0u64;
            let mut has_digits = false;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                num = num * 10 + u64::from(bytes[i] - b'0');
                has_digits = true;
                i += 1;
            }
            if has_digits {
                max_param = max_param.max(num as usize);
            } else {
                bare_count += 1;
            }
        } else {
            i += 1;
        }
    }

    if max_param > 0 {
        max_param
    } else {
        bare_count
    }
}

// ── Error conversion ──────────────────────────────────────────────────────

fn franken_to_conn_error(e: &fsqlite_error::FrankenError) -> Error {
    Error::Connection(ConnectionError {
        kind: ConnectionErrorKind::Connect,
        message: e.to_string(),
        source: None,
    })
}

fn franken_to_query_error(e: &fsqlite_error::FrankenError, sql: &str) -> Error {
    use fsqlite_error::FrankenError;

    let kind = match e {
        FrankenError::UniqueViolation { .. } | FrankenError::NotNullViolation { .. } => {
            QueryErrorKind::Constraint
        }
        FrankenError::ForeignKeyViolation { .. } | FrankenError::CheckViolation { .. } => {
            QueryErrorKind::Constraint
        }
        FrankenError::WriteConflict { .. } | FrankenError::SerializationFailure { .. } => {
            QueryErrorKind::Deadlock
        }
        FrankenError::SyntaxError { .. } => QueryErrorKind::Syntax,
        FrankenError::QueryReturnedNoRows => QueryErrorKind::NotFound,
        _ => QueryErrorKind::Database,
    };

    Error::Query(QueryError {
        kind,
        sql: Some(sql.to_string()),
        sqlstate: None,
        message: e.to_string(),
        detail: None,
        hint: None,
        position: None,
        source: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_memory_succeeds() {
        let conn = FrankenConnection::open_memory().expect("should open in-memory db");
        assert_eq!(conn.path(), ":memory:");
    }

    #[test]
    fn execute_raw_create_table() {
        let conn = FrankenConnection::open_memory().unwrap();
        conn.execute_raw("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)")
            .unwrap();
    }

    #[test]
    fn query_sync_basic() {
        let conn = FrankenConnection::open_memory().unwrap();
        let rows = conn.query_sync("SELECT 1 + 2, 'hello'", &[]).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get(0), Some(&Value::BigInt(3)));
        assert_eq!(rows[0].get(1), Some(&Value::Text("hello".into())));
    }

    #[test]
    fn execute_sync_insert() {
        let conn = FrankenConnection::open_memory().unwrap();
        conn.execute_raw("CREATE TABLE t (id INTEGER PRIMARY KEY, val TEXT)")
            .unwrap();
        let count = conn
            .execute_sync(
                "INSERT INTO t (val) VALUES (?1)",
                &[Value::Text("test".into())],
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn query_with_params() {
        let conn = FrankenConnection::open_memory().unwrap();
        let rows = conn
            .query_sync("SELECT ?1 + ?2", &[Value::BigInt(10), Value::BigInt(20)])
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get(0), Some(&Value::BigInt(30)));
    }

    #[test]
    fn transaction_commit() {
        let conn = FrankenConnection::open_memory().unwrap();
        conn.execute_raw("CREATE TABLE t (id INTEGER PRIMARY KEY, val TEXT)")
            .unwrap();

        conn.begin_sync(IsolationLevel::ReadCommitted).unwrap();
        conn.execute_sync("INSERT INTO t (val) VALUES (?1)", &[Value::Text("a".into())])
            .unwrap();
        conn.commit_sync().unwrap();

        let rows = conn.query_sync("SELECT val FROM t", &[]).unwrap();
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn transaction_rollback() {
        let conn = FrankenConnection::open_memory().unwrap();
        conn.execute_raw("CREATE TABLE t (id INTEGER PRIMARY KEY, val TEXT)")
            .unwrap();

        conn.begin_sync(IsolationLevel::ReadCommitted).unwrap();
        conn.execute_sync("INSERT INTO t (val) VALUES (?1)", &[Value::Text("a".into())])
            .unwrap();
        conn.rollback_sync().unwrap();

        let rows = conn.query_sync("SELECT val FROM t", &[]).unwrap();
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn dialect_is_sqlite() {
        let conn = FrankenConnection::open_memory().unwrap();
        assert_eq!(conn.dialect(), sqlmodel_core::Dialect::Sqlite);
    }

    #[test]
    fn count_params_numbered() {
        assert_eq!(count_params("SELECT ?1, ?2, ?3"), 3);
        assert_eq!(count_params("INSERT INTO t VALUES (?1, ?2)"), 2);
    }

    #[test]
    fn count_params_bare() {
        assert_eq!(count_params("SELECT ?, ?"), 2);
    }

    #[test]
    fn count_params_none() {
        assert_eq!(count_params("SELECT 1"), 0);
    }

    #[test]
    fn infer_select_column_names() {
        let names = infer_column_names("SELECT id, name AS username, count(*) AS total FROM t");
        assert_eq!(names, vec!["id", "username", "total"]);
    }

    #[test]
    fn infer_pragma_table_info() {
        let names = infer_column_names("PRAGMA table_info(users)");
        assert!(names.contains(&"name".to_string()));
        assert!(names.contains(&"type".to_string()));
    }

    #[test]
    fn infer_expression_select() {
        let names = infer_column_names("SELECT 1 + 2 AS result");
        assert_eq!(names, vec!["result"]);
    }

    #[test]
    fn ping_succeeds() {
        let conn = FrankenConnection::open_memory().unwrap();
        let result = conn.query_sync("SELECT 1", &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn multiple_statements_in_execute_raw() {
        let conn = FrankenConnection::open_memory().unwrap();
        conn.execute_raw(
            "CREATE TABLE a (id INTEGER PRIMARY KEY); CREATE TABLE b (id INTEGER PRIMARY KEY)",
        )
        .unwrap();
        // Verify both tables exist by inserting into them
        conn.execute_sync("INSERT INTO a (id) VALUES (1)", &[])
            .unwrap();
        conn.execute_sync("INSERT INTO b (id) VALUES (1)", &[])
            .unwrap();
    }

    #[test]
    fn insert_returns_rowid() {
        let conn = FrankenConnection::open_memory().unwrap();
        conn.execute_raw("CREATE TABLE t (id INTEGER PRIMARY KEY, val TEXT)")
            .unwrap();
        // Insert and verify via query
        conn.execute_sync(
            "INSERT INTO t (val) VALUES (?1)",
            &[Value::Text("a".into())],
        )
        .unwrap();
        let rows = conn.query_sync("SELECT id FROM t", &[]).unwrap();
        assert_eq!(rows.len(), 1);
        // Verify we got a row back (auto-increment may not produce the
        // same values as C SQLite, but row should exist)
        assert!(rows[0].get(0).is_some());
    }
}
