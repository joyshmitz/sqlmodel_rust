//! Database migration support.

use asupersync::{Cx, Outcome};
use sqlmodel_core::{Connection, Error, Value};
use std::collections::HashMap;

/// A database migration.
#[derive(Debug, Clone)]
pub struct Migration {
    /// Unique migration ID (typically timestamp-based)
    pub id: String,
    /// Human-readable description
    pub description: String,
    /// SQL to apply the migration
    pub up: String,
    /// SQL to revert the migration
    pub down: String,
}

impl Migration {
    /// Create a new migration.
    pub fn new(
        id: impl Into<String>,
        description: impl Into<String>,
        up: impl Into<String>,
        down: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            up: up.into(),
            down: down.into(),
        }
    }
}

/// Status of a migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationStatus {
    /// Migration has not been applied
    Pending,
    /// Migration has been applied
    Applied { at: i64 },
    /// Migration failed
    Failed { error: String },
}

/// Migration runner for executing migrations.
pub struct MigrationRunner {
    /// The migrations to manage
    migrations: Vec<Migration>,
    /// Name of the migrations tracking table
    table_name: String,
}

impl MigrationRunner {
    /// Create a new migration runner with the given migrations.
    pub fn new(migrations: Vec<Migration>) -> Self {
        Self {
            migrations,
            table_name: "_sqlmodel_migrations".to_string(),
        }
    }

    /// Set a custom migrations tracking table name.
    pub fn table_name(mut self, name: impl Into<String>) -> Self {
        self.table_name = name.into();
        self
    }

    /// Ensure the migrations tracking table exists.
    pub async fn init<C: Connection>(&self, cx: &Cx, conn: &C) -> Outcome<(), Error> {
        let sql = format!(
            "CREATE TABLE IF NOT EXISTS {} (
                id TEXT PRIMARY KEY,
                description TEXT NOT NULL,
                applied_at INTEGER NOT NULL
            )",
            self.table_name
        );

        conn.execute(cx, &sql, &[]).await.map(|_| ())
    }

    /// Get the status of all migrations.
    pub async fn status<C: Connection>(
        &self,
        cx: &Cx,
        conn: &C,
    ) -> Outcome<Vec<(String, MigrationStatus)>, Error> {
        // First ensure table exists
        match self.init(cx, conn).await {
            Outcome::Ok(()) => {}
            Outcome::Err(e) => return Outcome::Err(e),
            Outcome::Cancelled(r) => return Outcome::Cancelled(r),
            Outcome::Panicked(p) => return Outcome::Panicked(p),
        }

        // Query applied migrations
        let sql = format!("SELECT id, applied_at FROM {}", self.table_name);
        let rows = match conn.query(cx, &sql, &[]).await {
            Outcome::Ok(rows) => rows,
            Outcome::Err(e) => return Outcome::Err(e),
            Outcome::Cancelled(r) => return Outcome::Cancelled(r),
            Outcome::Panicked(p) => return Outcome::Panicked(p),
        };

        let mut applied: HashMap<String, i64> = HashMap::new();
        for row in rows {
            if let (Ok(id), Ok(at)) = (
                row.get_named::<String>("id"),
                row.get_named::<i64>("applied_at"),
            ) {
                applied.insert(id, at);
            }
        }

        let status: Vec<_> = self
            .migrations
            .iter()
            .map(|m| {
                let status = if let Some(&at) = applied.get(&m.id) {
                    MigrationStatus::Applied { at }
                } else {
                    MigrationStatus::Pending
                };
                (m.id.clone(), status)
            })
            .collect();

        Outcome::Ok(status)
    }

    /// Apply all pending migrations.
    pub async fn migrate<C: Connection>(&self, cx: &Cx, conn: &C) -> Outcome<Vec<String>, Error> {
        let status = match self.status(cx, conn).await {
            Outcome::Ok(s) => s,
            Outcome::Err(e) => return Outcome::Err(e),
            Outcome::Cancelled(r) => return Outcome::Cancelled(r),
            Outcome::Panicked(p) => return Outcome::Panicked(p),
        };

        let mut applied = Vec::new();

        for (id, s) in status {
            if s == MigrationStatus::Pending {
                let migration = self.migrations.iter().find(|m| m.id == id).unwrap();

                // Execute the up migration
                match conn.execute(cx, &migration.up, &[]).await {
                    Outcome::Ok(_) => {}
                    Outcome::Err(e) => return Outcome::Err(e),
                    Outcome::Cancelled(r) => return Outcome::Cancelled(r),
                    Outcome::Panicked(p) => return Outcome::Panicked(p),
                }

                // Record the migration
                let record_sql = format!(
                    "INSERT INTO {} (id, description, applied_at) VALUES ($1, $2, $3)",
                    self.table_name
                );
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64;

                match conn
                    .execute(
                        cx,
                        &record_sql,
                        &[
                            Value::Text(migration.id.clone()),
                            Value::Text(migration.description.clone()),
                            Value::BigInt(now),
                        ],
                    )
                    .await
                {
                    Outcome::Ok(_) => {}
                    Outcome::Err(e) => return Outcome::Err(e),
                    Outcome::Cancelled(r) => return Outcome::Cancelled(r),
                    Outcome::Panicked(p) => return Outcome::Panicked(p),
                }

                applied.push(id);
            }
        }

        Outcome::Ok(applied)
    }

    /// Rollback the last applied migration.
    pub async fn rollback<C: Connection>(
        &self,
        cx: &Cx,
        conn: &C,
    ) -> Outcome<Option<String>, Error> {
        let status = match self.status(cx, conn).await {
            Outcome::Ok(s) => s,
            Outcome::Err(e) => return Outcome::Err(e),
            Outcome::Cancelled(r) => return Outcome::Cancelled(r),
            Outcome::Panicked(p) => return Outcome::Panicked(p),
        };

        // Find the last applied migration
        let last_applied = status
            .iter()
            .filter_map(|(id, s)| {
                if let MigrationStatus::Applied { at } = s {
                    Some((id.clone(), *at))
                } else {
                    None
                }
            })
            .max_by_key(|(_, at)| *at);

        let Some((id, _)) = last_applied else {
            return Outcome::Ok(None);
        };

        let migration = self.migrations.iter().find(|m| m.id == id).unwrap();

        // Execute the down migration
        match conn.execute(cx, &migration.down, &[]).await {
            Outcome::Ok(_) => {}
            Outcome::Err(e) => return Outcome::Err(e),
            Outcome::Cancelled(r) => return Outcome::Cancelled(r),
            Outcome::Panicked(p) => return Outcome::Panicked(p),
        }

        // Remove the migration record
        let delete_sql = format!("DELETE FROM {} WHERE id = $1", self.table_name);
        match conn
            .execute(cx, &delete_sql, &[Value::Text(id.clone())])
            .await
        {
            Outcome::Ok(_) => {}
            Outcome::Err(e) => return Outcome::Err(e),
            Outcome::Cancelled(r) => return Outcome::Cancelled(r),
            Outcome::Panicked(p) => return Outcome::Panicked(p),
        }

        Outcome::Ok(Some(id))
    }
}
